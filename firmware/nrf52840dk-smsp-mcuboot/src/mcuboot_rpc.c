#include "mcuboot_rpc.h"

#include <ctype.h>
#include <errno.h>
#include <string.h>
#include <zephyr/dfu/flash_img.h>
#include <zephyr/dfu/mcuboot.h>
#include <zephyr/storage/flash_map.h>
#include <zcbor_decode.h>
#include <zcbor_encode.h>

#define RPC_HDR_MAX 512
#define SHA256_HEX_LEN 64
#define IMAGE_MAGIC 0x96f3b83d
#define IMAGE_TLV_INFO_MAGIC 0x6907
#define IMAGE_TLV_PROT_INFO_MAGIC 0x6908
#define IMAGE_TLV_SHA256 0x10
#define IMAGE_HASH_LEN 32

struct image_header {
	uint32_t ih_magic;
	uint32_t ih_load_addr;
	uint16_t ih_hdr_size;
	uint16_t ih_protect_tlv_size;
	uint32_t ih_img_size;
	uint32_t ih_flags;
	uint8_t ih_ver_major;
	uint8_t ih_ver_minor;
	uint16_t ih_ver_revision;
	uint32_t ih_ver_build_num;
	uint32_t ih_pad1;
} __packed;

struct image_tlv_info {
	uint16_t it_magic;
	uint16_t it_tlv_tot;
} __packed;

struct image_tlv {
	uint16_t it_type;
	uint16_t it_len;
} __packed;

struct upload_ctx {
	struct flash_img_context flash;
	bool active;
	uint32_t slot;
	char sha256[SHA256_HEX_LEN + 1];
};

static struct upload_ctx upload;

static int hex_nibble(char c)
{
	if (c >= '0' && c <= '9') {
		return c - '0';
	}
	if (c >= 'a' && c <= 'f') {
		return c - 'a' + 10;
	}
	if (c >= 'A' && c <= 'F') {
		return c - 'A' + 10;
	}
	return -1;
}

static int hex_to_bin(const char *hex, uint8_t *out, size_t out_len)
{
	if (strlen(hex) != out_len * 2) {
		return -1;
	}
	for (size_t i = 0; i < out_len; i++) {
		int hi = hex_nibble(hex[i * 2]);
		int lo = hex_nibble(hex[i * 2 + 1]);

		if (hi < 0 || lo < 0) {
			return -1;
		}
		out[i] = (uint8_t)((hi << 4) | lo);
	}
	return 0;
}

static void bin_to_hex_lower(const uint8_t *bin, size_t bin_len, char *hex, size_t hex_cap)
{
	static const char digits[] = "0123456789abcdef";

	if (hex_cap < bin_len * 2 + 1) {
		hex[0] = '\0';
		return;
	}
	for (size_t i = 0; i < bin_len; i++) {
		hex[i * 2] = digits[bin[i] >> 4];
		hex[i * 2 + 1] = digits[bin[i] & 0x0f];
	}
	hex[bin_len * 2] = '\0';
}

static int flash_area_read_id(uint8_t area_id, off_t off, void *dst, size_t len)
{
	const struct flash_area *fa;
	int rc;

	rc = flash_area_open(area_id, &fa);
	if (rc != 0) {
		return rc;
	}
	rc = flash_area_read(fa, off, dst, len);
	flash_area_close(fa);
	return rc;
}

static int find_image_tlv_area(uint8_t area_id, size_t *start_off, size_t *end_off, uint16_t magic)
{
	struct image_tlv_info tlv_info;
	int rc;

	rc = flash_area_read_id(area_id, *start_off, &tlv_info, sizeof(tlv_info));
	if (rc != 0) {
		return rc;
	}
	if (tlv_info.it_magic != magic) {
		return -ENOENT;
	}
	*start_off += sizeof(tlv_info);
	*end_off = *start_off + tlv_info.it_tlv_tot;
	return 0;
}

static int read_image_sha256(uint8_t area_id, uint8_t hash[IMAGE_HASH_LEN])
{
	struct image_header hdr;
	struct image_tlv tlv;
	size_t data_off;
	size_t data_end;
	bool hash_found;
	int rc;

	rc = flash_area_read_id(area_id, boot_get_image_start_offset(area_id), &hdr, sizeof(hdr));
	if (rc != 0) {
		return rc;
	}
	if (hdr.ih_magic != IMAGE_MAGIC) {
		return -EINVAL;
	}
	data_off = boot_get_image_start_offset(area_id) + hdr.ih_hdr_size + hdr.ih_img_size;
	rc = find_image_tlv_area(area_id, &data_off, &data_end, IMAGE_TLV_PROT_INFO_MAGIC);
	if (rc == 0) {
		data_off = data_end - sizeof(struct image_tlv_info);
	}
	rc = find_image_tlv_area(area_id, &data_off, &data_end, IMAGE_TLV_INFO_MAGIC);
	if (rc != 0) {
		return rc;
	}
	hash_found = false;
	while (data_off + sizeof(tlv) <= data_end) {
		rc = flash_area_read_id(area_id, data_off, &tlv, sizeof(tlv));
		if (rc != 0) {
			return rc;
		}
		if (tlv.it_type == 0xff && tlv.it_len == 0xffff) {
			return -EINVAL;
		}
		if (tlv.it_type != IMAGE_TLV_SHA256 || tlv.it_len != IMAGE_HASH_LEN) {
			data_off += sizeof(tlv) + tlv.it_len;
			continue;
		}
		if (hash_found) {
			return -EINVAL;
		}
		hash_found = true;
		data_off += sizeof(tlv);
		return flash_area_read_id(area_id, data_off, hash, IMAGE_HASH_LEN);
	}
	return hash_found ? 0 : -ENOENT;
}

static int encode_frame(const uint8_t *hdr, size_t hdr_len, const uint8_t *data, size_t data_len,
			uint8_t *out, size_t out_cap, size_t *out_len)
{
	if (hdr_len > UINT16_MAX || out_cap < 2 + hdr_len + data_len) {
		return -1;
	}
	out[0] = (uint8_t)(hdr_len >> 8);
	out[1] = (uint8_t)(hdr_len & 0xff);
	memcpy(out + 2, hdr, hdr_len);
	if (data_len > 0) {
		memcpy(out + 2 + hdr_len, data, data_len);
	}
	*out_len = 2 + hdr_len + data_len;
	return 0;
}

static int encode_error(const char *for_type, int code, const char *msg, uint8_t *out, size_t out_cap,
			size_t *out_len)
{
	uint8_t hdr[RPC_HDR_MAX];
	zcbor_state_t state[4];
	size_t hdr_len;

	zcbor_new_encode_state(state, ARRAY_SIZE(state), hdr, sizeof(hdr), 0);
	if (!zcbor_map_start_encode(state, 4) || !zcbor_tstr_put_lit(state, "type") ||
	    !zcbor_tstr_put_lit(state, "error") || !zcbor_tstr_put_lit(state, "for")) {
		return -1;
	}
	if (for_type) {
		if (!zcbor_tstr_encode_ptr(state, for_type, strlen(for_type))) {
			return -1;
		}
	} else if (!zcbor_tstr_put_lit(state, "")) {
		return -1;
	}
	if (!zcbor_tstr_put_lit(state, "code") || !zcbor_int32_put(state, code) ||
	    !zcbor_tstr_put_lit(state, "message") ||
	    !zcbor_tstr_encode_ptr(state, msg, strlen(msg)) ||
	    !zcbor_map_end_encode(state, 4)) {
		return -1;
	}
	hdr_len = (size_t)(state[0].payload - hdr);
	return encode_frame(hdr, hdr_len, NULL, 0, out, out_cap, out_len);
}

static bool map_key_match(struct zcbor_string *key, const char *lit)
{
	size_t len = strlen(lit);

	return key->len == len && memcmp(key->value, lit, len) == 0;
}

static int decode_upload_fields(const uint8_t *hdr, size_t hdr_len, uint32_t *slot, char *sha, size_t sha_cap,
				uint64_t *total, uint64_t *offset, uint64_t *length)
{
	zcbor_state_t state[4];
	struct zcbor_string key;
	bool found_type;

	zcbor_new_decode_state(state, ARRAY_SIZE(state), (uint8_t *)hdr, hdr_len, 0, NULL, 0);
	if (!zcbor_map_start_decode(state)) {
		return -1;
	}
	found_type = false;
	for (;;) {
		if (zcbor_map_end_decode(state)) {
			break;
		}
		if (!zcbor_tstr_decode(state, &key)) {
			return -1;
		}
		if (map_key_match(&key, "type")) {
			struct zcbor_string type_val;

			if (!zcbor_tstr_decode(state, &type_val) || type_val.len != 12 ||
			    memcmp(type_val.value, "upload_image", 12) != 0) {
				return -1;
			}
			found_type = true;
		} else if (map_key_match(&key, "slot")) {
			int32_t v;

			if (!zcbor_int32_decode(state, &v)) {
				return -1;
			}
			*slot = (uint32_t)v;
		} else if (map_key_match(&key, "sha256")) {
			struct zcbor_string s;

			if (!zcbor_tstr_decode(state, &s) || s.len >= sha_cap) {
				return -1;
			}
			memcpy(sha, s.value, s.len);
			sha[s.len] = '\0';
		} else if (map_key_match(&key, "total")) {
			uint64_t v;

			if (!zcbor_uint64_decode(state, &v)) {
				return -1;
			}
			*total = v;
		} else if (map_key_match(&key, "offset")) {
			uint64_t v;

			if (!zcbor_uint64_decode(state, &v)) {
				return -1;
			}
			*offset = v;
		} else if (map_key_match(&key, "length")) {
			uint64_t v;

			if (!zcbor_uint64_decode(state, &v)) {
				return -1;
			}
			*length = v;
		} else if (!zcbor_any_skip(state, NULL)) {
			return -1;
		}
	}
	return found_type ? 0 : -1;
}

static int decode_type_list_images(const uint8_t *hdr, size_t hdr_len)
{
	static const char lit[] = "list_images";

	if (hdr_len < sizeof(lit) - 1) {
		return -1;
	}
	for (size_t i = 0; i + sizeof(lit) - 1 <= hdr_len; i++) {
		if (memcmp(hdr + i, lit, sizeof(lit) - 1) == 0) {
			return 0;
		}
	}
	return -1;
}

static uint8_t slot_to_area_id(uint32_t slot)
{
	return slot == 0 ? PARTITION_ID(slot1_partition) : PARTITION_ID(slot0_partition);
}

static int slot_image_count(void)
{
	struct mcuboot_img_header hdr;
	int count = 0;

	if (boot_read_bank_header(PARTITION_ID(slot0_partition), &hdr, sizeof(hdr)) == 0) {
		count++;
	}
	if (boot_read_bank_header(PARTITION_ID(slot1_partition), &hdr, sizeof(hdr)) == 0) {
		count++;
	}
	return count;
}

static int append_image(zcbor_state_t *state, uint8_t area_id, uint32_t slot_idx)
{
	struct mcuboot_img_header hdr;
	uint8_t hash_bin[IMAGE_HASH_LEN];
	char hash_hex[SHA256_HEX_LEN + 1];
	int err = boot_read_bank_header(area_id, &hdr, sizeof(hdr));
	bool active = boot_fetch_active_slot() == area_id;

	if (err != 0) {
		return 0;
	}
	hash_hex[0] = '\0';
	if (read_image_sha256(area_id, hash_bin) == 0) {
		bin_to_hex_lower(hash_bin, sizeof(hash_bin), hash_hex, sizeof(hash_hex));
	}
	if (!zcbor_map_start_encode(state, 4) || !zcbor_tstr_put_lit(state, "slot") ||
	    !zcbor_int32_put(state, (int32_t)slot_idx) || !zcbor_tstr_put_lit(state, "version") ||
	    !zcbor_int32_put(state, (int32_t)hdr.h.v1.sem_ver.build_num) ||
	    !zcbor_tstr_put_lit(state, "hash") ||
	    !zcbor_tstr_encode_ptr(state, hash_hex, strlen(hash_hex)) ||
	    !zcbor_tstr_put_lit(state, "active") || !zcbor_bool_put(state, active) ||
	    !zcbor_map_end_encode(state, 4)) {
		return -1;
	}
	return 1;
}

static int handle_list_images(uint8_t *out, size_t out_cap, size_t *out_len)
{
	uint8_t hdr[RPC_HDR_MAX];
	zcbor_state_t state[8];
	size_t hdr_len;
	int image_count = slot_image_count();

	zcbor_new_encode_state(state, ARRAY_SIZE(state), hdr, sizeof(hdr), 0);
	if (!zcbor_map_start_encode(state, 2) || !zcbor_tstr_put_lit(state, "type") ||
	    !zcbor_tstr_put_lit(state, "list_images") || !zcbor_tstr_put_lit(state, "images") ||
	    !zcbor_list_start_encode(state, image_count)) {
		return -1;
	}
	if (append_image(state, PARTITION_ID(slot0_partition), 0) < 0 ||
	    append_image(state, PARTITION_ID(slot1_partition), 1) < 0) {
		return -1;
	}
	if (!zcbor_list_end_encode(state, image_count) || !zcbor_map_end_encode(state, 2)) {
		return -1;
	}
	hdr_len = (size_t)(state[0].payload - hdr);
	return encode_frame(hdr, hdr_len, NULL, 0, out, out_cap, out_len);
}

static int finalize_upload(uint8_t area_id)
{
	uint8_t hash_bin[32];
	struct flash_img_check fic = {0};

	if (flash_img_buffered_write(&upload.flash, NULL, 0, true) != 0) {
		return -1;
	}
	if (hex_to_bin(upload.sha256, hash_bin, sizeof(hash_bin)) != 0) {
		return -1;
	}
	fic.match = hash_bin;
	fic.clen = sizeof(hash_bin);
	return flash_img_check(&upload.flash, &fic, area_id);
}

static int handle_upload_image(const uint8_t *rpc_hdr, size_t rpc_hdr_len, const uint8_t *data, size_t data_len,
			       uint8_t *out, size_t out_cap, size_t *out_len)
{
	uint32_t slot = 0;
	char sha[SHA256_HEX_LEN + 1];
	uint64_t total = 0;
	uint64_t offset = 0;
	uint64_t length = 0;
	uint8_t area_id;
	size_t written;
	uint8_t hdr[RPC_HDR_MAX];
	zcbor_state_t state[4];
	uint64_t remaining;
	size_t hdr_len;

	if (decode_upload_fields(rpc_hdr, rpc_hdr_len, &slot, sha, sizeof(sha), &total, &offset, &length) != 0) {
		return encode_error("upload_image", 2, "invalid header", out, out_cap, out_len);
	}
	if (length != data_len) {
		return encode_error("upload_image", 4, "length mismatch", out, out_cap, out_len);
	}
	area_id = slot_to_area_id(slot);
	if (offset == 0) {
		if (flash_img_init_id(&upload.flash, area_id) != 0) {
			return encode_error("upload_image", 2, "flash init failed", out, out_cap, out_len);
		}
		upload.active = true;
		upload.slot = slot;
		strncpy(upload.sha256, sha, sizeof(upload.sha256) - 1);
	} else if (!upload.active || upload.slot != slot || strncmp(upload.sha256, sha, sizeof(upload.sha256)) != 0) {
		return encode_error("upload_image", 3, "offset mismatch", out, out_cap, out_len);
	}
	written = flash_img_bytes_written(&upload.flash);
	if (offset != written) {
		return encode_error("upload_image", 3, "offset mismatch", out, out_cap, out_len);
	}
	if (data_len > 0 && flash_img_buffered_write(&upload.flash, data, data_len, false) != 0) {
		return encode_error("upload_image", 2, "flash write failed", out, out_cap, out_len);
	}
	written = flash_img_bytes_written(&upload.flash);
	remaining = total > written ? total - written : 0;
	if (remaining == 0 && total > 0) {
		if (finalize_upload(area_id) != 0) {
			return encode_error("upload_image", 2, "hash mismatch", out, out_cap, out_len);
		}
		upload.active = false;
	}
	zcbor_new_encode_state(state, ARRAY_SIZE(state), hdr, sizeof(hdr), 0);
	if (!zcbor_map_start_encode(state, 3) || !zcbor_tstr_put_lit(state, "type") ||
	    !zcbor_tstr_put_lit(state, "upload_image") || !zcbor_tstr_put_lit(state, "offset") ||
	    !zcbor_uint64_put(state, written) || !zcbor_tstr_put_lit(state, "remaining") ||
	    !zcbor_uint64_put(state, remaining) || !zcbor_map_end_encode(state, 3)) {
		return -1;
	}
	hdr_len = (size_t)(state[0].payload - hdr);
	return encode_frame(hdr, hdr_len, NULL, 0, out, out_cap, out_len);
}

int mcuboot_rpc_handle(const uint8_t *frame, size_t frame_len, uint8_t *out, size_t out_cap, size_t *out_len)
{
	uint16_t hdr_len;
	const uint8_t *rpc_hdr;
	const uint8_t *data;
	size_t data_len;

	if (frame_len < 2) {
		return encode_error(NULL, 2, "invalid frame", out, out_cap, out_len);
	}
	hdr_len = ((uint16_t)frame[0] << 8) | frame[1];
	if ((size_t)2 + hdr_len > frame_len || hdr_len > RPC_HDR_MAX) {
		return encode_error(NULL, 2, "invalid frame", out, out_cap, out_len);
	}
	rpc_hdr = frame + 2;
	data = frame + 2 + hdr_len;
	data_len = frame_len - 2 - hdr_len;
	if (decode_type_list_images(rpc_hdr, hdr_len) == 0) {
		return handle_list_images(out, out_cap, out_len);
	}
	{
		uint32_t slot = 0;
		char sha[SHA256_HEX_LEN + 1];
		uint64_t total = 0;
		uint64_t offset = 0;
		uint64_t length = 0;

		if (decode_upload_fields(rpc_hdr, hdr_len, &slot, sha, sizeof(sha), &total, &offset, &length) == 0) {
			return handle_upload_image(rpc_hdr, hdr_len, data, data_len, out, out_cap, out_len);
		}
	}
	return encode_error(NULL, 1, "unknown type", out, out_cap, out_len);
}
