#![no_std]
#![no_main]

use core::fmt::Write;

use panic_semihosting as _;
use stm32c0xx_hal as hal;

use hal::gpio::*;
use hal::prelude::*;
use hal::spi::*;
use hal::stm32;
use hal::timer::*;
use hal::analog::adc::{self, Adc};

use ssd1306::{mode, prelude::*, Ssd1306};

use klaptik::*;

struct AppState {
    adc_val: u16,
    mv_val: u16,
}

pub struct App {
    state: AppState,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState {
                adc_val: 0,
                mv_val: 0,
            },
        }
    }

    fn state(&self) -> &AppState {
        &self.state
    }

    fn update(&mut self, adc: u16, mv: u16) {
        self.state.adc_val = adc;
        self.state.mv_val = mv;
    }
}

enum Asset {
    Background = 0,
    Numbers = 1,
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
        raw_value: Label<4>, Asset::Numbers, "0000", Point::new(8*5, 8*2), Size::new(16, 16);
        mv_value: Label<4>, Asset::Numbers, "0000", Point::new(8*5, 8*5), Size::new(16, 16);
    },
    |widget: &mut UI, state: &AppState| {
        write!(widget.raw_value, "{: >4}", state.adc_val).ok();
        write!(widget.mv_value, "{: >4}", state.mv_val).ok();
    }
}

type SPII = SPIInterface<
    Spi<hal::pac::SPI, (PA5<DefaultMode>, NoMiso, PA7<DefaultMode>)>,
    PA9<Output<PushPull>>,
    PA15<Output<PushPull>>,
>;
type DisplayDriver = Ssd1306<SPII, DisplaySize128x64, mode::BasicMode>;
struct DisplayController {
    canvas: DisplayDriver,
}

impl DisplayController {
    fn new(canvas: DisplayDriver) -> Self {
        Self { canvas }
    }
}

impl Canvas for DisplayController {
    fn draw(&mut self, bounds: Rectangle, bitmap: &[u8]) {
        let (start, end) = (bounds.start(), bounds.end());
        self.canvas
            .set_draw_area((start.x, start.y), (end.x, end.y))
            .unwrap();
        self.canvas.draw(bitmap).unwrap();
    }
}

pub const SPRITES: [(FlashSprite, Glyphs); 2] = [
    (
        FlashSprite::new(
            Asset::Background as _,
            1,
            Size::new(128, 64),
            include_bytes!("adc.bin"),
        ),
        Glyphs::Sequential(1),
    ),
    (
        FlashSprite::new(
            Asset::Numbers as _,
            11,
            Size::new(16, 16),
            include_bytes!("numbers16x16.bin"),
        ),
        Glyphs::Alphabet(b" 0123456789"),
    ),
];

#[rtic::app(device = stm32, peripherals = true)]
mod app {
    use super::*;

    #[shared]
    struct Shared {
        app: App,
    }

    #[local]
    struct Local {
        adc: Adc,
        display: SpriteDisplay<DisplayController, { SPRITES.len() }>,
        ui: UI,
        ui_timer: Timer<stm32::TIM17>,
        pot_input: PA0<DefaultMode>,
    }

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

        let mut ui_timer = ctx.device.TIM17.timer(&mut rcc);
        ui_timer.start(350.millis());
        ui_timer.listen();

        let mut adc = ctx.device.ADC.constrain(&mut rcc);
        adc.set_sample_time(adc::SampleTime::T_160);
        adc.set_precision(adc::Precision::B_12);
        adc.set_oversampling_ratio(adc::OversamplingRatio::X_16);
        adc.set_oversampling_shift(20);
        adc.oversampling_enable(true);

        let spi = ctx.device.SPI.spi(
            (sck, NoMiso, mosi),
            Mode {
                polarity: Polarity::IdleLow,
                phase: Phase::CaptureOnFirstTransition,
            },
            2.MHz(),
            &mut rcc,
        );

        let interface = display_interface_spi::SPIInterface::new(spi, dc, nss);
        let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0);

        let pot_input = gpio_a.pa0;
        adc.calibrate();

        let mut delay = ctx.device.TIM3.delay(&mut rcc);
        display.reset(&mut rst, &mut delay).unwrap();

        display.init().unwrap();
        let controller = DisplayController::new(display);
        let display = SpriteDisplay::new(controller, SPRITES);
        let ui = UI::new();

        let app = App::new();

        (
            Shared { app },
            Local {
                adc,
                display,
                ui,
                ui_timer,
                pot_input,
            },
            init::Monotonics(),
        )
    }

    #[task(binds = TIM17, local = [adc, ui, ui_timer, display, pot_input], shared = [app])]
    fn ui_timer_tick(ctx: ui_timer_tick::Context) {
        let mut app = ctx.shared.app;
        let pot_raw: u16 = ctx.local.adc.read(ctx.local.pot_input).unwrap_or(0);
        let pot_mv: u16 = ctx.local.adc.read_voltage(ctx.local.pot_input).unwrap_or(0);
        app.lock(|app| {
            app.update(pot_raw, pot_mv);
            ctx.local.ui.update(app.state());
        });
        ctx.local.ui.render(ctx.local.display);
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        loop {
            cortex_m::asm::wfi();
        }
    }
}
