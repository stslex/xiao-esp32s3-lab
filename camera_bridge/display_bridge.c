#include "camera_bridge.h"

#include <string.h>
#include <stdlib.h>
#include "driver/spi_master.h"
#include "esp_heap_caps.h"
#include "esp_lcd_io_spi.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_vendor.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"

#define WIDTH 240
#define HEIGHT 320
#define STRIPE_ROWS 8
#define STRIPE_BYTES (WIDTH * STRIPE_ROWS * 2)

// Rust serializes all calls. DMA owns the stripe until its completion callback.
static esp_lcd_panel_handle_t panel;
static esp_lcd_panel_io_handle_t io;
static SemaphoreHandle_t transfer_done;
static uint8_t *stripe;
static bool faulted;

static bool on_transfer_done(esp_lcd_panel_io_handle_t handle,
                             esp_lcd_panel_io_event_data_t *event, void *context) {
    BaseType_t woken = pdFALSE;
    xSemaphoreGiveFromISR(transfer_done, &woken);
    return woken == pdTRUE;
}

int xiao_display_init(int cs, int dc, int reset, int mosi, int clock) {
    if (panel != NULL) return ESP_ERR_INVALID_STATE;
    const spi_bus_config_t bus = {
        .mosi_io_num = mosi, .miso_io_num = -1, .sclk_io_num = clock,
        .quadwp_io_num = -1, .quadhd_io_num = -1,
        .max_transfer_sz = STRIPE_BYTES,
    };
    esp_err_t result = spi_bus_initialize(SPI2_HOST, &bus, SPI_DMA_CH_AUTO);
    if (result != ESP_OK) return result;

    transfer_done = xSemaphoreCreateBinary();
    stripe = heap_caps_malloc(STRIPE_BYTES, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
    if (transfer_done == NULL || stripe == NULL) {
        result = ESP_ERR_NO_MEM;
        goto fail;
    }
    const esp_lcd_panel_io_spi_config_t config = {
        .cs_gpio_num = cs, .dc_gpio_num = dc, .spi_mode = 0,
        .pclk_hz = 10000000, .trans_queue_depth = 1,
        .on_color_trans_done = on_transfer_done,
        .lcd_cmd_bits = 8, .lcd_param_bits = 8,
    };
    result = esp_lcd_new_panel_io_spi(SPI2_HOST, &config, &io);
    if (result != ESP_OK) goto fail;
    const esp_lcd_panel_dev_config_t device = {
        .reset_gpio_num = reset,
        .rgb_ele_order = LCD_RGB_ELEMENT_ORDER_RGB,
        .data_endian = LCD_RGB_DATA_ENDIAN_BIG,
        .bits_per_pixel = 16,
    };
    result = esp_lcd_new_panel_st7789(io, &device, &panel);
    if (result != ESP_OK) goto fail;
    result = esp_lcd_panel_reset(panel);
    if (result != ESP_OK) goto fail;
    result = esp_lcd_panel_init(panel);
    if (result != ESP_OK) goto fail;
    result = esp_lcd_panel_invert_color(panel, true);
    if (result != ESP_OK) goto fail;
    // The full 240x320 panel has no cropped-area offsets.
    result = esp_lcd_panel_disp_on_off(panel, true);
    if (result == ESP_OK) return result;

fail:
    if (panel) { esp_lcd_panel_del(panel); panel = NULL; }
    if (io) { esp_lcd_panel_io_del(io); io = NULL; }
    if (stripe) { free(stripe); stripe = NULL; }
    if (transfer_done) { vSemaphoreDelete(transfer_done); transfer_done = NULL; }
    spi_bus_free(SPI2_HOST);
    return result;
}

int xiao_display_draw(const uint8_t *rgb565_be, size_t length) {
    if (!panel || faulted) return ESP_ERR_INVALID_STATE;
    if (!rgb565_be || length != WIDTH * HEIGHT * 2) return ESP_ERR_INVALID_ARG;
    for (int y = 0; y < HEIGHT; y += STRIPE_ROWS) {
        memcpy(stripe, rgb565_be + y * WIDTH * 2, STRIPE_BYTES);
        esp_err_t result = esp_lcd_panel_draw_bitmap(panel, 0, y, WIDTH, y + STRIPE_ROWS, stripe);
        if (result != ESP_OK) {
            faulted = true;
            return result;
        }
        if (xSemaphoreTake(transfer_done, pdMS_TO_TICKS(1000)) != pdTRUE) {
            // Keep DMA storage alive and never reuse it after uncertain completion.
            faulted = true;
            return ESP_ERR_TIMEOUT;
        }
    }
    return ESP_OK;
}
