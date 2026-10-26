#ifndef MCUBOOT_RPC_H_
#define MCUBOOT_RPC_H_

#include <stddef.h>
#include <stdint.h>

int mcuboot_rpc_handle(const uint8_t *frame, size_t frame_len, uint8_t *out, size_t out_cap,
		       size_t *out_len);

#endif
