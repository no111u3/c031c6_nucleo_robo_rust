#![no_std]
#![no_main]

use panic_semihosting as _;
use stm32c0xx_hal as hal;

use hal::gpio::*;
use hal::prelude::*;
use hal::spi;
use hal::stm32;

use ssd1306::{prelude::*, Ssd1306};

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};

#[rtic::app(device = stm32, peripherals = true)]
mod app {
    use super::*;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {}

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        let mut rcc = ctx.device.RCC.constrain();
        let gpio_a = ctx.device.GPIOA.split(&mut rcc);

        // Setup spi i/o
        let sck = gpio_a.pa5;
        let mosi = gpio_a.pa7;
        let mut nss = gpio_a.pa15.into_push_pull_output();
        nss.set_high().ok();
        let mut dc = gpio_a.pa9.into_push_pull_output();
        dc.set_high().ok();

        let spi = ctx.device.SPI.spi(
            (sck, spi::NoMiso, mosi),
            spi::Mode {
                polarity: spi::Polarity::IdleLow,
                phase: spi::Phase::CaptureOnFirstTransition,
            },
            500.kHz(),
            &mut rcc,
        );

        let interface = display_interface_spi::SPIInterface::new(spi, dc, nss);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        display.init().unwrap();

        let text_style = MonoTextStyleBuilder::new()
            .font(&FONT_6X10)
            .text_color(BinaryColor::On)
            .build();

        Text::with_baseline("Hello world!", Point::zero(), text_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();

        Text::with_baseline("Hello Rust!", Point::new(0, 16), text_style, Baseline::Top)
            .draw(&mut display)
            .unwrap();

        display.flush().unwrap();

        (Shared {}, Local {}, init::Monotonics())
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        loop {
            cortex_m::asm::wfi();
        }
    }
}
