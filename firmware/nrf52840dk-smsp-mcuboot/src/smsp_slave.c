#include "smsp_slave.h"

#include <dk_buttons_and_leds.h>
#include <string.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/kernel.h>

#define SMSP_INIT_PREFIX "parameters?"
#define SMSP_PARAMS "1,1,20000,1,256\n"

enum smsp_status {
	SMSP_LAST = 0,
	SMSP_MORE = 1,
	SMSP_CANCEL = 2,
};

#define UART_RX_Q 512
static uint8_t rx_q[UART_RX_Q];
static volatile uint16_t rx_w;
static volatile uint16_t rx_r;

static void uart_rx_isr(const struct device *dev, void *user_data)
{
	ARG_UNUSED(user_data);

	while (uart_irq_update(dev) && uart_irq_rx_ready(dev)) {
		uint8_t c;
		uint16_t next;

		if (uart_fifo_read(dev, &c, 1) != 1) {
			break;
		}
		next = (uint16_t)((rx_w + 1U) % UART_RX_Q);
		if (next == rx_r) {
			continue;
		}
		rx_q[rx_w] = c;
		rx_w = next;
	}
}

static void uart_rx_start(const struct device *uart)
{
	rx_w = 0;
	rx_r = 0;
	uart_irq_callback_set(uart, uart_rx_isr);
	uart_irq_rx_enable(uart);
}

static int uart_read_byte(const struct device *uart, uint8_t *byte, int32_t timeout_ms)
{
	ARG_UNUSED(uart);
	for (int32_t i = 0; i < timeout_ms; i++) {
		if (rx_r != rx_w) {
			*byte = rx_q[rx_r];
			rx_r = (uint16_t)((rx_r + 1U) % UART_RX_Q);
			return 0;
		}
		k_msleep(1);
	}
	return -1;
}

static int uart_write_all(const struct device *uart, const uint8_t *data, size_t len)
{
	uart_irq_rx_disable(uart);
	for (size_t i = 0; i < len; i++) {
		uart_poll_out(uart, data[i]);
	}
	uart_err_check(uart);
	uart_irq_rx_enable(uart);
	return 0;
}

static int read_exact(const struct device *uart, uint8_t *buf, size_t len)
{
	for (size_t i = 0; i < len; i++) {
		if (uart_read_byte(uart, &buf[i], 5000) != 0) {
			return -1;
		}
	}
	return 0;
}

static int parse_header(const uint8_t *hdr, enum smsp_status *st, uint8_t *seq, uint16_t *plen)
{
	*st = hdr[0] & 0x03;
	*seq = (hdr[0] >> 4) & 0x0f;
	if ((hdr[0] & 0x04) == 0) {
		return -1;
	}
	*plen = ((uint16_t)hdr[1] << 8) | hdr[2];
	return 0;
}

static int send_header(const struct device *uart, enum smsp_status st, uint8_t seq, const uint8_t *payload,
		       size_t payload_len)
{
	uint8_t hdr[3];
	hdr[0] = (uint8_t)st | 0x04 | (seq << 4);
	hdr[1] = (uint8_t)(payload_len >> 8);
	hdr[2] = (uint8_t)(payload_len & 0xff);
	uart_write_all(uart, hdr, sizeof(hdr));
	if (payload_len > 0) {
		uart_write_all(uart, payload, payload_len);
	}
	return 0;
}

static int send_ack(const struct device *uart)
{
	return send_header(uart, SMSP_LAST, 0, NULL, 0);
}

static int read_message(const struct device *uart, uint8_t *buf, size_t cap, size_t *out_len, bool *is_last)
{
	uint8_t hdr[3];
	enum smsp_status st;
	uint8_t seq;
	uint16_t plen;
	size_t total = 0;
	uint8_t expect = 0;

	*out_len = 0;
	for (;;) {
		if (read_exact(uart, hdr, sizeof(hdr)) != 0) {
			return -1;
		}
		if (parse_header(hdr, &st, &seq, &plen) != 0) {
			return -1;
		}
		if (seq != expect) {
			return -1;
		}
		expect = (expect + 1) & 0x0f;
		if (total + plen > cap) {
			return -1;
		}
		if (plen > 0 && read_exact(uart, buf + total, plen) != 0) {
			return -1;
		}
		total += plen;
		if (st == SMSP_CANCEL) {
			return -1;
		}
		if (st == SMSP_LAST) {
			*is_last = true;
			*out_len = total;
			return 0;
		}
		send_ack(uart);
	}
}

static int send_fragments(const struct device *uart, const uint8_t *data, size_t len)
{
	size_t off = 0;
	uint8_t seq = 0;

	while (off < len) {
		size_t chunk = len - off;
		if (chunk > SMSP_FRAG_MAX) {
			chunk = SMSP_FRAG_MAX;
		}
		bool last = (off + chunk) >= len;
		enum smsp_status st = last ? SMSP_LAST : SMSP_MORE;
		if (send_header(uart, st, seq, data + off, chunk) != 0) {
			return -1;
		}
		off += chunk;
		seq = (seq + 1) & 0x0f;
		if (!last) {
			uint8_t ack_hdr[3];
			if (read_exact(uart, ack_hdr, sizeof(ack_hdr)) != 0) {
				return -1;
			}
			enum smsp_status ast;
			uint8_t aseq;
			uint16_t alen;
			if (parse_header(ack_hdr, &ast, &aseq, &alen) != 0 || ast != SMSP_LAST || alen != 0) {
				return -1;
			}
		}
	}
	return 0;
}

static int wait_init(const struct device *uart)
{
	const size_t plen = sizeof(SMSP_INIT_PREFIX) - 1;

	for (;;) {
		size_t i = 0;

		while (i < plen) {
			uint8_t b;

			if (uart_read_byte(uart, &b, 60000) != 0) {
				return -1;
			}
			if (b != (uint8_t)SMSP_INIT_PREFIX[i]) {
				i = (b == (uint8_t)SMSP_INIT_PREFIX[0]) ? 1 : 0;
				continue;
			}
			i++;
		}
		{
			uint8_t b;

			if (uart_read_byte(uart, &b, 200) != 0 || b == 0) {
				break;
			}
			if (b == (uint8_t)SMSP_INIT_PREFIX[0]) {
				continue;
			}
		}
	}
	if (uart_write_all(uart, (const uint8_t *)SMSP_PARAMS, strlen(SMSP_PARAMS)) != 0) {
		return -1;
	}
	dk_set_led_on(DK_LED4);
	k_msleep(50);
	return 0;
}

int smsp_slave_run(const struct device *uart, smsp_request_fn handler)
{
	static uint8_t req[SMSP_RX_MAX];
	static uint8_t resp[SMSP_TX_MAX];
	size_t req_len;
	size_t resp_len;
	bool is_last;

	if (!device_is_ready(uart)) {
		return -1;
	}
	uart_rx_start(uart);
	for (;;) {
		if (wait_init(uart) != 0) {
			continue;
		}
		if (read_message(uart, req, sizeof(req), &req_len, &is_last) != 0) {
			continue;
		}
		resp_len = 0;
		if (handler(req, req_len, resp, sizeof(resp), &resp_len) != 0) {
			continue;
		}
		if (send_fragments(uart, resp, resp_len) != 0) {
			continue;
		}
	}
}
