idf_component_register(
    SRCS "camera_bridge.c" "display_bridge.c" "photo_bridge.c"
    INCLUDE_DIRS "include"
    REQUIRES esp32-camera esp_lcd esp_driver_spi esp_partition esp_jpeg
)
