#include "camera_bridge.h"

#include "esp_camera.h"
#include "driver/gpio.h"
#include <unistd.h>

int xiao_usb_read(uint8_t *buffer, size_t length) {
    // The default ESP-IDF USB console VFS reads without blocking.
    return (int)read(STDIN_FILENO, buffer, length);
}

int xiao_usb_write(const uint8_t *buffer, size_t length) {
    // One VFS write holds the USB port lock, also used by stdout/stderr logs.
    // Its default polling backend fails fast if the host stops reading.
    return (int)write(STDOUT_FILENO, buffer, length);
}

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
        .frame_size = FRAMESIZE_QXGA,
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
        return xiao_camera_apply(2, 12, 0, 0, 0, 0, 1);
    }
    return ESP_ERR_INVALID_STATE;
}

int xiao_camera_apply(int resolution_index, int quality, int brightness, int contrast,
                      int saturation, int hmirror, int vflip) {
    static const framesize_t sizes[] = {FRAMESIZE_QVGA, FRAMESIZE_VGA, FRAMESIZE_SVGA,
        FRAMESIZE_XGA, FRAMESIZE_SXGA, FRAMESIZE_UXGA, FRAMESIZE_QXGA};
    sensor_t *sensor = esp_camera_sensor_get();
    if (!sensor) return ESP_ERR_INVALID_STATE;
    if (resolution_index < 0 || resolution_index >= 7 || quality < 10 || quality > 40 ||
        brightness < -2 || brightness > 2 || contrast < -2 || contrast > 2 ||
        saturation < -2 || saturation > 2 || hmirror < 0 || hmirror > 1 || vflip < 0 || vflip > 1) {
        return ESP_ERR_INVALID_ARG;
    }
    if (sensor->set_framesize(sensor, sizes[resolution_index]) ||
        sensor->set_quality(sensor, quality) || sensor->set_brightness(sensor, brightness) ||
        sensor->set_contrast(sensor, contrast) || sensor->set_saturation(sensor, saturation) ||
        sensor->set_hmirror(sensor, hmirror) || sensor->set_vflip(sensor, vflip)) {
        return ESP_FAIL;
    }
    return ESP_OK;
}

int xiao_camera_suspend(void) {
    int result = esp_camera_deinit();
    if (result == ESP_OK) {
        // The S3 driver leaves the XCLK matrix connection configured after deinit.
        // Disconnect it and hold the camera clock low until the next initialization.
        gpio_reset_pin(GPIO_NUM_10);
        gpio_set_direction(GPIO_NUM_10, GPIO_MODE_OUTPUT);
        gpio_set_level(GPIO_NUM_10, 0);
    }
    return result;
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
