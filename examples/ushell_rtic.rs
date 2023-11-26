#![no_std]
#![no_main]
#![deny(warnings)]

use rtic::{self, Mutex};

use panic_semihosting as _;
use stm32c0xx_hal as hal;

use hal::exti::Event;
use hal::gpio::*;
use hal::prelude::*;
use hal::stm32;
use hal::time::*;
use hal::timer::*;
use hal::serial::*;

use core::fmt::Write;

use defmt_rtt as _;

mod shell {
    use super::*;

    pub use ushell::{
        autocomplete::StaticAutocomplete, control, history::LRUHistory, Environment,
        Input as ushell_input, ShellError as ushell_error, SpinResult, UShell,
    };

    pub const CMD_MAX_LEN: usize = 32;

    pub type Autocomplete = StaticAutocomplete<5>;
    pub type History = LRUHistory<{ CMD_MAX_LEN }, 32>;
    pub type Uart = Serial<stm32::USART2>;
    pub type Shell = UShell<Uart, Autocomplete, History, { CMD_MAX_LEN }>;

    pub enum EnvSignal {
        Shell,
        ButtonClick,
    }

    pub type Env<'a> = super::app::env::SharedResources<'a>;
    pub type EnvResult = SpinResult<Uart, ()>;

    impl Env<'_> {
        pub fn on_signal(&mut self, shell: &mut Shell, sig: EnvSignal) -> EnvResult {
            match sig {
                EnvSignal::Shell => shell.spin(self),
                EnvSignal::ButtonClick => self.button_click(),
            }
        }

        fn button_click(&mut self) -> EnvResult {
            self.timer.lock(|tim| {
                if tim.enabled() {
                    tim.pause();
                } else {
                    tim.resume();
                }
            });
            Ok(())
        }

        fn status_cmd(&mut self, shell: &mut Shell) -> EnvResult {
            self.timer.lock(|tim| {
                if tim.enabled() {
                    write!(shell, "{0:}Led enabled{0:}\r\n", CR).unwrap();
                } else {
                    write!(shell, "{0:}Led disabled{0:}\r\n", CR).unwrap();
                }
            });
            Ok(())
        }

        fn off_cmd(&mut self, shell: &mut Shell) -> EnvResult {
            self.timer.lock(|tim| {
                if tim.enabled() {
                    write!(shell, "{0:}Led disabled{0:}\r\n", CR).unwrap();
                    tim.pause();
                } else {
                    write!(shell, "{0:}Led already off{0:}\r\n", CR).unwrap();
                }
            });
            Ok(())
        }

        fn on_cmd(&mut self, shell: &mut Shell) -> EnvResult {
            self.timer.lock(|tim| {
                if tim.enabled() {
                    write!(shell, "{0:}Led already on{0:}\r\n", CR).unwrap();
                } else {
                    tim.resume();
                    write!(shell, "{0:}Led enabled: {0:}\r\n", CR).unwrap();
                }
            });
            Ok(())
        }


        fn help_cmd(&mut self, shell: &mut Shell, args: &str) -> EnvResult {
            match args {
                _ => shell.write_str(HELP)?,
            }
            Ok(())
        }
    }

    impl Environment<Uart, Autocomplete, History, (), { CMD_MAX_LEN }> for Env<'_> {
        fn command(&mut self, shell: &mut Shell, cmd: &str, args: &str) -> EnvResult {
            match cmd {
                "clear" => shell.clear()?,
                "status" => self.status_cmd(shell)?,
                "on" => self.on_cmd(shell)?,
                "off" => self.off_cmd(shell)?,
                "help" => self.help_cmd(shell, args)?,
                "" => shell.write_str(CR)?,
                _ => write!(shell, "{0:}unsupported command: \"{1:}\"{0:}", CR, cmd)?,
            }
            shell.write_str(SHELL_PROMPT)?;
            Ok(())
        }

        fn control(&mut self, shell: &mut Shell, code: u8) -> EnvResult {
            match code {
                control::CTRL_C => {
                    shell.write_str(CR)?;
                    shell.write_str(SHELL_PROMPT)?;
                }
                _ => {}
            }
            Ok(())
        }
    }

    pub const AUTOCOMPLETE: Autocomplete =
        StaticAutocomplete(["clear", "help", "off", "on", "status"]);

    const SHELL_PROMPT: &str = "#> ";
    const CR: &str = "\r\n";
    const HELP: &str = "\r\n\
LED Shell v.1\r\n\r\n\
USAGE:\r\n\
\tcommand\r\n\r\n\
COMMANDS:\r\n\
\ton        Enable led\r\n\
\toff       Disable led\r\n\
\tstatus    Get led status\r\n\
\tclear     Clear screen\r\n\
\thelp      Print this message\r\n\
";
}

#[rtic::app(device = stm32, peripherals = true, dispatchers = [USART1])]
mod app {
    use super::*;

    #[shared]
    struct Shared {
        timer: Timer<stm32::TIM17>,
    }

    #[local]
    struct Local {
        exti: stm32::EXTI,
        led: PA5<Output<PushPull>>,
        shell: shell::Shell,
    }

    #[init]
    fn init(ctx: init::Context) -> (Shared, Local, init::Monotonics) {
        let mut rcc = ctx.device.RCC.constrain();
        let gpio_a = ctx.device.GPIOA.split(&mut rcc);
        let gpio_c = ctx.device.GPIOC.split(&mut rcc);

        let mut timer = ctx.device.TIM17.timer(&mut rcc);
        timer.start(Hertz::Hz(3).into_duration());
        timer.listen();

        let mut exti = ctx.device.EXTI;
        gpio_c.pc13.listen(SignalEdge::Falling, &mut exti);

        let mut serial = ctx
            .device
            .USART2
            .usart((gpio_a.pa2, gpio_a.pa3), Config::default(), &mut rcc)
            .unwrap();
        serial.listen(hal::serial::Event::Rxne);

        writeln!(serial, "Hello from STM32C031\r\n").unwrap();

        // shell
        let shell =
            shell::UShell::new(serial, shell::AUTOCOMPLETE, shell::LRUHistory::default());

        (
            Shared { timer },
            Local {
                exti,
                led: gpio_a.pa5.into_push_pull_output(),
                shell
            },
            init::Monotonics(),
        )
    }

    #[task(binds = TIM17, shared = [timer], local = [led])]
    fn timer_tick(mut ctx: timer_tick::Context) {
        ctx.local.led.toggle().ok();
        ctx.shared.timer.lock(|tim| tim.clear_irq());
    }

    #[task(binds = USART2, priority = 1)]
    fn serial_callback(_: serial_callback::Context) {
        env::spawn(shell::EnvSignal::Shell).ok();
    }

    #[task(priority = 2, capacity = 8, local = [shell], shared = [timer])]
    fn env(ctx: env::Context, sig: shell::EnvSignal) {
        let mut env = ctx.shared;
        env.on_signal(ctx.local.shell, sig).ok();
    }

    #[task(binds = EXTI4_15, local = [exti])]
    fn button_click(ctx: button_click::Context) {
        env::spawn(shell::EnvSignal::ButtonClick).ok();
        ctx.local.exti.unpend(Event::GPIO13);
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        loop {
            cortex_m::asm::wfi();
        }
    }
}
