#include "camera_bridge.h"
#include "esp_partition.h"
#include "esp_heap_caps.h"
#include "jpeg_decoder.h"
#include <stdlib.h>
#include <string.h>

static const esp_partition_t *photo_partition(void) {
    // The standard data subtype keeps the table compatible with espflash.
    // This partition stores raw CRC-protected records; no filesystem is mounted.
    return esp_partition_find_first(ESP_PARTITION_TYPE_DATA, ESP_PARTITION_SUBTYPE_DATA_SPIFFS, "photo");
}

int xiao_photo_read(size_t offset, uint8_t *data, size_t length) {
    const esp_partition_t *p = photo_partition();
    return p ? esp_partition_read(p, offset, data, length) : ESP_ERR_NOT_FOUND;
}

int xiao_photo_write(size_t offset, const uint8_t *data, size_t length) {
    const esp_partition_t *p = photo_partition();
    return p ? esp_partition_write(p, offset, data, length) : ESP_ERR_NOT_FOUND;
}

int xiao_photo_erase(size_t offset, size_t length) {
    const esp_partition_t *p = photo_partition();
    return p ? esp_partition_erase_range(p, offset, length) : ESP_ERR_NOT_FOUND;
}

int xiao_photo_decode(const uint8_t *jpeg, size_t length, uint8_t *pixels, size_t capacity, uint16_t *width, uint16_t *height) {
    if (!jpeg || !pixels || !width || !height || length > 262144) return ESP_ERR_INVALID_ARG;
    esp_jpeg_image_cfg_t cfg = {
        .indata = (uint8_t *)jpeg, .indata_size = length,
        .out_format = JPEG_IMAGE_FORMAT_RGB888, .out_scale = JPEG_IMAGE_SCALE_0,
    };
    esp_jpeg_image_output_t info = {0};
    esp_err_t result = esp_jpeg_get_image_info(&cfg, &info);
    if (result != ESP_OK) return result;
    if (!info.width || !info.height || info.width > 480 || info.height > 480 ||
        (size_t)info.width * info.height * 2 > capacity ||
        info.output_len != (size_t)info.width * info.height * 3) return ESP_ERR_INVALID_SIZE;
    uint8_t *rgb = heap_caps_malloc(info.output_len, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (!rgb) return ESP_ERR_NO_MEM;
    cfg.outbuf = rgb;
    cfg.outbuf_size = info.output_len;
    result = esp_jpeg_decode(&cfg, &info);
    if (result == ESP_OK) {
        *width = info.width;
        *height = info.height;
        // Return uncropped source pixels. Rust handles all framing and rotation.
        for (size_t i = 0; i < (size_t)info.width * info.height; i++) {
            uint16_t color = ((rgb[i*3] & 248) << 8) | ((rgb[i*3+1] & 252) << 3) | (rgb[i*3+2] >> 3);
            pixels[i*2] = color >> 8;
            pixels[i*2+1] = color & 255;
        }
    }
    free(rgb);
    return result;
}
