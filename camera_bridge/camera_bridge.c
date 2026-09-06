#include "camera_bridge.h"

#include "esp_camera.h"

int xiao_camera_init(void) {
    camera_config_t config = {
        .pin_pwdn = -1,
        .pin_reset = -1,
        .pin_xclk = 10,
        .pin_sccb_sda = 40,
        .pin_sccb_scl = 39,
        .pin_d7 = 48,
        .pin_d6 = 11,
        .pin_d5 = 12,
        .pin_d4 = 14,
        .pin_d3 = 16,
        .pin_d2 = 18,
        .pin_d1 = 17,
        .pin_d0 = 15,
        .pin_vsync = 38,
        .pin_href = 47,
        .pin_pclk = 13,
        .xclk_freq_hz = 20000000,
        .ledc_timer = LEDC_TIMER_0,
        .ledc_channel = LEDC_CHANNEL_0,
        .pixel_format = PIXFORMAT_JPEG,
        .frame_size = FRAMESIZE_QVGA,
        .jpeg_quality = 12,
        .fb_count = 1,
        .fb_location = CAMERA_FB_IN_PSRAM,
        .grab_mode = CAMERA_GRAB_WHEN_EMPTY,
        .sccb_i2c_port = -1,
    };

    const esp_err_t result = esp_camera_init(&config);
    if (result != ESP_OK) {
        return result;
    }

    sensor_t *sensor = esp_camera_sensor_get();
    if (sensor != NULL) {
        sensor->set_vflip(sensor, 1);
    }

    return ESP_OK;
}

void *xiao_camera_capture(void) {
    return esp_camera_fb_get();
}

const uint8_t *xiao_camera_frame_data(void *frame) {
    const camera_fb_t *camera_frame = (const camera_fb_t *)frame;
    return camera_frame == NULL ? NULL : camera_frame->buf;
}

size_t xiao_camera_frame_length(void *frame) {
    const camera_fb_t *camera_frame = (const camera_fb_t *)frame;
    return camera_frame == NULL ? 0 : camera_frame->len;
}

void xiao_camera_release(void *frame) {
    if (frame != NULL) {
        esp_camera_fb_return((camera_fb_t *)frame);
    }
}
