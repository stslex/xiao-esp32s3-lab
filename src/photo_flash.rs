use crate::photo_record::Storage;
use esp_idf_svc::{hal::delay::FreeRtos, sys::camera};
pub struct Flash;
impl Storage for Flash {
    fn read(&mut self, offset: usize, data: &mut [u8]) -> Result<(), &'static str> {
        if unsafe { camera::xiao_photo_read(offset, data.as_mut_ptr(), data.len()) } == 0 {
            Ok(())
        } else {
            Err("photo_storage_read_failed_check_partition_table")
        }
    }
    fn write(&mut self, offset: usize, data: &[u8]) -> Result<(), &'static str> {
        for (i, chunk) in data.chunks(4096).enumerate() {
            if unsafe { camera::xiao_photo_write(offset + i * 4096, chunk.as_ptr(), chunk.len()) }
                != 0
            {
                return Err("photo_storage_write_failed");
            }
            FreeRtos::delay_ms(10);
        }
        Ok(())
    }
    fn erase(&mut self, offset: usize, length: usize) -> Result<(), &'static str> {
        for i in (0..length).step_by(4096) {
            if unsafe { camera::xiao_photo_erase(offset + i, 4096) } != 0 {
                return Err("photo_storage_erase_failed");
            }
            FreeRtos::delay_ms(10);
        }
        Ok(())
    }
}
