#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int xiao_camera_init(void);
int xiao_camera_suspend(void);
int xiao_camera_apply(int resolution_index, int quality, int brightness, int contrast,
                      int saturation, int hmirror, int vflip);
int xiao_usb_read(uint8_t *buffer, size_t length);
int xiao_usb_write(const uint8_t *buffer, size_t length);
void *xiao_camera_capture(void);
const uint8_t *xiao_camera_frame_data(void *frame);
size_t xiao_camera_frame_length(void *frame);
void xiao_camera_release(void *frame);
int xiao_display_init(int cs, int dc, int reset, int mosi, int clock);
int xiao_display_draw(const uint8_t *rgb565_be, size_t length);
// Raw records live in the photo partition (data/SPIFFS subtype, no mounted filesystem).
int xiao_photo_read(size_t offset, uint8_t *data, size_t length);
int xiao_photo_write(size_t offset, const uint8_t *data, size_t length);
int xiao_photo_erase(size_t offset, size_t length);
int xiao_photo_decode(const uint8_t *jpeg, size_t length, uint8_t *pixels, size_t capacity, uint16_t *width, uint16_t *height);

#ifdef __cplusplus
}
#endif
