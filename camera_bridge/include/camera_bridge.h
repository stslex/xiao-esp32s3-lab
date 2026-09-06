#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

int xiao_camera_init(void);
void *xiao_camera_capture(void);
const uint8_t *xiao_camera_frame_data(void *frame);
size_t xiao_camera_frame_length(void *frame);
void xiao_camera_release(void *frame);

#ifdef __cplusplus
}
#endif
