#![no_main]
#![no_std]

use ariel_os::debug::log::info;
use ariel_os::spi::main::SpiDevice;
use ariel_os::time::{Delay, Timer};
use ariel_os::{gpio, hal, i2c, spi};
use embassy_sync::mutex::Mutex;
use embedded_hal_async::i2c::I2c as _;
use ssd1680_rs::config::{LUTSelect, UpdateRamOption, VDBMode};

const BQ25895_ADDR: u16 = 0x6A;
const EXPANDER_ADDR: u16 = 0x20;
mod pins;

#[ariel_os::task(autostart, peripherals)]
async fn main(peripherals: pins::I2cBus) {
    info!("Hello World!");

    // set up i2c bus
    let mut i2c_config = hal::i2c::controller::Config::default();
    i2c_config.frequency = const {
        i2c::controller::highest_freq_in(
            i2c::controller::Kilohertz::kHz(100)..=i2c::controller::Kilohertz::kHz(400),
        )
    };

    let i2c_bus = pins::SensorI2c::new(peripherals.i2c_sda, peripherals.i2c_scl, i2c_config);
    let mut i2c_device = i2c_bus;

    const TARGET_I2C_ADDR: u8 = 0x6A;
    const WHO_AM_I_REG_ADDR: u8 = 0x14;

    ///    let mut i2c_device = I2cDevice::new(&i2c_bus);
    let mut id = [0];
    i2c_device
        .write_read(TARGET_I2C_ADDR, &[WHO_AM_I_REG_ADDR], &mut id)
        .await
        .unwrap();

    let who_am_i = id[0];
    info!("WHO_AM_I_COMMAND register value: 0x{:x}", who_am_i);
    assert_eq!(who_am_i & 0b111111, 0x39);

    // set reg 4 to 0x08 (512mAh fast charging rate)
    i2c_device
        .write(TARGET_I2C_ADDR, &[0x04, 0x08])
        .await
        .unwrap();

    let mut tmp = [0x0];
    i2c_device
        .write_read(TARGET_I2C_ADDR, &[0x03], &mut tmp)
        .await
        .unwrap();

    // set reg 3[1-3] to 0b101 (3.3v min voltage)
    let reg03_target = (tmp[0] & 0b11110001) | (0b101 << 1);
    i2c_device
        .write(TARGET_I2C_ADDR, &[0x03, reg03_target])
        .await
        .unwrap();

    let mut tmp = [0x0];
    i2c_device
        .write_read(TARGET_I2C_ADDR, &[0x03], &mut tmp)
        .await
        .unwrap();
    info!("0x03 register value: 0b{:b}", tmp[0]);
}

const FRAME_BUFFER_SIZE: usize = 128 * 296;

#[ariel_os::task(autostart, peripherals)]
async fn screen(peripherals: pins::Epd) {
    static SPI_BUS: once_cell::sync::OnceCell<
        Mutex<embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, hal::spi::main::Spi>,
    > = once_cell::sync::OnceCell::new();

    let mut spi_config = hal::spi::main::Config::default();
    spi_config.frequency = const {
        spi::main::highest_freq_in(
            spi::main::Kilohertz::kHz(1000)..=spi::main::Kilohertz::kHz(2000),
        )
    };

    let spi_bus = pins::EpdSpi::new(
        peripherals.spi_sck,
        peripherals.spi_miso,
        peripherals.spi_mosi,
        spi_config,
    );

    let _ = SPI_BUS.set(Mutex::new(spi_bus));

    let cs_output = gpio::Output::new(peripherals.spi_cs, gpio::Level::High);
    let dc = gpio::Output::new(peripherals.dc, gpio::Level::High);
    let busy = gpio::Input::builder(peripherals.busy, gpio::Pull::Up)
        .build_with_interrupt()
        .unwrap();
    let reset = gpio::Output::new(peripherals.reset, gpio::Level::High);

    let spi_device = SpiDevice::new(SPI_BUS.get().unwrap(), cs_output);

    // let config = ssd1680_rs::config::DisplayConfig {
    //     width: 128,
    //     height: 296,
    //     gate_scanning_gd: false,
    //     gate_scanning_sm: false,
    //     gate_scanning_tb: false,
    //     partial_refresh_sequence: 0xFC,
    //     full_refresh_sequence: 0xF7,
    //     border_waveform_control: VDBMode::GSTransition(true, LUTSelect::LUT1),
    //     ram_content_for_display_update: UpdateRamOption::Normal,
    //     s8_source_output_mode: true,
    //     use_internal_temperature_sensor: true,
    // };

    let config = ssd1680_rs::config::DisplayConfig::epd_290_t94();

    let mut epd_controller =
        ssd1680_rs::driver_async::SSD1680::new(reset, dc, busy, Delay, spi_device, config);

    let mut frame_buffer: [u8; 37888];

    loop {
        frame_buffer = [0u8; FRAME_BUFFER_SIZE];

        epd_controller.hw_init().await.unwrap();
        // epd_controller.fill_bw_screen(true).await.unwrap();

        epd_controller.write_bw_bytes(&frame_buffer).await.unwrap();
        epd_controller.full_refresh().await.unwrap();
        epd_controller.enter_deep_sleep().await.unwrap();
        Timer::after_millis(500).await;
        frame_buffer = [255u8; FRAME_BUFFER_SIZE];

        epd_controller.hw_init().await.unwrap();

        epd_controller.write_bw_bytes(&frame_buffer).await.unwrap();

        epd_controller.full_refresh().await.unwrap();
        epd_controller.enter_deep_sleep().await.unwrap();
    }
}
