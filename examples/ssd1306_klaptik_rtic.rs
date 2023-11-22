#![no_std]
#![no_main]

use panic_semihosting as _;
use stm32c0xx_hal as hal;

use hal::gpio::*;
use hal::prelude::*;
use hal::spi;
use hal::stm32;

use ssd1306::{prelude::*, Ssd1306};

use klaptik::*;
struct AppState {}

enum Asset {
    Background = 0,
}

impl From<Asset> for SpriteId {
    fn from(asset: Asset) -> Self {
        asset as _
    }
}

widget_group! {
    UI<&AppState>,
    {
        bg: GlyphIcon, Asset::Background, 0, Point::zero();
    },
    |_: &mut UI, _: &AppState| {

    }
}

type DisplayDriver<DI, Size, Mode> = Ssd1306<DI, Size, Mode>;
struct DisplayController<DI, Size, Mode> {
    canvas: DisplayDriver<DI, Size, Mode>,
}

impl<DI, Size, Mode> DisplayController<DI, Size, Mode> {
    fn new(canvas: DisplayDriver<DI, Size, Mode>) -> Self {
        Self { canvas }
    }
}

impl<DI: WriteOnlyDataCommand, Size: DisplaySize, Mode> Canvas
    for DisplayController<DI, Size, Mode>
{
    fn draw(&mut self, bounds: Rectangle, bitmap: &[u8]) {
        let (start, end) = (bounds.start(), bounds.end());
        self.canvas
            .set_draw_area((start.x, start.y), (end.x, end.y))
            .unwrap();
        self.canvas.draw(bitmap).unwrap();
    }
}

pub const SPRITES: [(FlashSprite, Glyphs); 1] = [(
    FlashSprite::new(
        Asset::Background as _,
        1,
        Size::new(128, 64),
        include_bytes!("background.bin"),
    ),
    Glyphs::Sequential(1),
)];

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
        let mut rst = gpio_a.pa10.into_push_pull_output();
        rst.set_high().ok();

        let spi = ctx.device.SPI.spi(
            (sck, spi::NoMiso, mosi),
            spi::Mode {
                polarity: spi::Polarity::IdleLow,
                phase: spi::Phase::CaptureOnFirstTransition,
            },
            2.MHz(),
            &mut rcc,
        );

        let interface = display_interface_spi::SPIInterface::new(spi, dc, nss);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0);

        let mut delay = ctx.device.TIM3.delay(&mut rcc);
        display.reset(&mut rst, &mut delay).unwrap();

        display.init().unwrap();
        let controller = DisplayController::new(display);
        let mut display = SpriteDisplay::new(controller, SPRITES);
        let mut ui = UI::new();

        ui.render(&mut display);

        (Shared {}, Local {}, init::Monotonics())
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        loop {
            cortex_m::asm::wfi();
        }
    }
}
