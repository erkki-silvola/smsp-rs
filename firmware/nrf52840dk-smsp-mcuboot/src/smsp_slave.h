#ifndef SMSP_SLAVE_H_
#define SMSP_SLAVE_H_

#include <stddef.h>
#include <stdint.h>
#include <zephyr/device.h>

#define SMSP_FRAG_MAX 256
#define SMSP_RX_MAX 8192
#define SMSP_TX_MAX 8192

typedef int (*smsp_request_fn)(const uint8_t *req, size_t req_len, uint8_t *resp,
			       size_t resp_cap, size_t *resp_len);

int smsp_slave_run(const struct device *uart, smsp_request_fn handler);

#endif
