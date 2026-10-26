#include "mcuboot_rpc.h"
#include "smsp_slave.h"

#include <dk_buttons_and_leds.h>
#include <zephyr/device.h>
#include <zephyr/drivers/uart.h>
#include <zephyr/kernel.h>

static int dispatch(const uint8_t *req, size_t req_len, uint8_t *resp, size_t resp_cap, size_t *resp_len)
{
	return mcuboot_rpc_handle(req, req_len, resp, resp_cap, resp_len);
}

int main(void)
{
	const struct device *uart = DEVICE_DT_GET(DT_CHOSEN(ncs_smsp_uart));

	dk_leds_init();
	dk_set_led_on(DK_LED1);
	for (int i = 0; i < 500 && !device_is_ready(uart); i++) {
		k_msleep(10);
	}
	if (!device_is_ready(uart)) {
		dk_set_led_off(DK_LED1);
		return 0;
	}
	dk_set_led_off(DK_LED1);
	dk_set_led_on(DK_LED2);
	smsp_slave_run(uart, dispatch);
	return 0;
}
