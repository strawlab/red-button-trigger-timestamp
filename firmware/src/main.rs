#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;
use rtic::Mutex;

use red_button_trigger_timestamp_comms::{FromDevice, ToDevice};

use json_lines::accumulator::{FeedResult, NewlinesAccumulator};

#[rtic::app(device = rp_pico::hal::pac, peripherals = true, dispatchers = [I2C0_IRQ])]
mod app {
    use super::*;
    use rp_pico::XOSC_CRYSTAL_FREQ;

    use heapless::spsc::{Consumer, Producer, Queue};
    use usb_device::{class_prelude::*, prelude::*};
    use usbd_serial::SerialPort;

    use embedded_hal::digital::v2::{InputPin, OutputPin};
    use rp2040_hal::{
        self as hal, clocks::init_clocks_and_plls, usb::UsbBus, watchdog::Watchdog, Sio,
    };
    use rp2040_monotonic::Rp2040Monotonic;

    const MAX_FRAME_SZ: usize = 256;
    const NUM_FRAMES: usize = 8;
    type UsbFrame = heapless::Vec<u8, MAX_FRAME_SZ>;

    #[shared]
    struct Shared {
        green_led: hal::gpio::Pin<
            hal::gpio::bank0::Gpio25,
            hal::gpio::FunctionSioOutput,
            hal::gpio::PullNone,
        >,
        usb_serial: SerialPort<'static, UsbBus>,
    }

    #[monotonic(binds = TIMER_IRQ_0, default = true)]
    type Monotonic = Rp2040Monotonic;

    #[local]
    struct Local {
        trigger_pin: hal::gpio::Pin<
            hal::gpio::bank0::Gpio15,
            hal::gpio::FunctionSioInput,
            hal::gpio::PullNone, // TODO: pullup?
        >,
        usb_dev: UsbDevice<'static, UsbBus>,
        rx_prod: Producer<'static, UsbFrame, NUM_FRAMES>,
        rx_cons: Consumer<'static, UsbFrame, NUM_FRAMES>,
    }

    #[init(local = [usb_bus: Option<UsbBusAllocator<UsbBus>> = None])]
    fn init(c: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::info!("Hello from {}.", env!["CARGO_PKG_NAME"]);
        let mut resets = c.device.RESETS;
        let mut watchdog = Watchdog::new(c.device.WATCHDOG);
        let clocks = init_clocks_and_plls(
            XOSC_CRYSTAL_FREQ,
            c.device.XOSC,
            c.device.CLOCKS,
            c.device.PLL_SYS,
            c.device.PLL_USB,
            &mut resets,
            &mut watchdog,
        )
        .ok()
        .unwrap();

        let usb_bus = c.local.usb_bus;
        usb_bus.replace(UsbBusAllocator::new(UsbBus::new(
            c.device.USBCTRL_REGS,
            c.device.USBCTRL_DPRAM,
            clocks.usb_clock,
            true,
            &mut resets,
        )));
        let usb_serial = SerialPort::new(usb_bus.as_ref().unwrap());

        let usb_dev = UsbDeviceBuilder::new(usb_bus.as_ref().unwrap(), UsbVidPid(0x16c0, 0x27dd))
            .manufacturer("Straw Lab")
            .product("Red Button Trigger Timestamp Logger")
            .serial_number("TEST")
            .device_class(2) // USB_CLASS_CDC
            .build();

        let sio = Sio::new(c.device.SIO);
        let pins = rp_pico::Pins::new(
            c.device.IO_BANK0,
            c.device.PADS_BANK0,
            sio.gpio_bank0,
            &mut resets,
        );

        let mut green_led = pins.led.reconfigure();
        green_led.set_low().unwrap();

        let trigger_pin = pins.gpio15.reconfigure();

        let rx_queue: &'static mut Queue<UsbFrame, NUM_FRAMES> = {
            static mut Q: Queue<UsbFrame, NUM_FRAMES> = Queue::new();
            unsafe { &mut Q }
        };
        let (rx_prod, rx_cons) = rx_queue.split();

        let mono = Monotonic::new(c.device.TIMER);

        (
            Shared {
                green_led,
                usb_serial,
            },
            Local {
                trigger_pin,
                usb_dev,
                rx_prod,
                rx_cons,
            },
            init::Monotonics(mono),
        )
    }

    fn send_response(
        response: &FromDevice,
        ctx: &mut idle::Context,
        &mut mut out_buf: &mut [u8; 256],
    ) {
        let encoded = json_lines::to_slice_newline(&response, &mut out_buf[..]).unwrap();

        ctx.shared.usb_serial.lock(|usb_serial| {
            usb_serial.write(&encoded).unwrap();
        });
        defmt::trace!("sent {} bytes", encoded.len());
    }

    #[idle(shared = [usb_serial, green_led], local = [trigger_pin, rx_cons])]
    fn idle(mut ctx: idle::Context) -> ! {
        let mut decoder = NewlinesAccumulator::<512>::new();
        let mut out_buf = [0u8; 256];

        let mut prev_state = ctx.local.trigger_pin.is_high().unwrap();
        loop {
            let this_state = ctx.local.trigger_pin.is_high().unwrap();
            if this_state != prev_state {
                if this_state == false {
                    let now = monotonics::Monotonic::now().ticks();
                    let response = FromDevice::Trigger(now);
                    send_response(&response, &mut ctx, &mut out_buf);
                }
                prev_state = this_state;
            }

            let frame = match ctx.local.rx_cons.dequeue() {
                Some(frame) => frame,
                None => continue,
            };
            let src = &frame.as_slice();

            let ret = match decoder.feed::<ToDevice>(src) {
                FeedResult::Consumed => None,
                FeedResult::OverFull(_remaining) => {
                    defmt::error!("frame overflow");
                    None
                }
                FeedResult::DeserError(_remaining) => {
                    defmt::error!("deserialization");
                    None
                }
                FeedResult::Success { data, remaining: _ } => Some(data),
            };

            if let Some(msg) = ret {
                let response;
                match msg {
                    ToDevice::Ping(val) => {
                        let now = monotonics::Monotonic::now().ticks();
                        response = FromDevice::Pong(val, now);
                        defmt::debug!("device state set");
                    }
                }
                send_response(&response, &mut ctx, &mut out_buf);
            }
        }
    }

    /// This function is called from the USB interrupt handler function (which
    /// does not have a return value). By here returning Result, we can abort
    /// processing early using idiomatic rust, even in the interrupt handler
    /// function.
    #[inline]
    fn on_usb_inner(
        usb_serial: &mut SerialPort<'static, UsbBus>,
        rx_prod: &mut Producer<'static, UsbFrame, NUM_FRAMES>,
    ) -> Result<usize, ()> {
        let mut new_frame = UsbFrame::new();
        new_frame.resize_default(MAX_FRAME_SZ)?;
        let new_frame_data = new_frame.as_mut_slice();

        match usb_serial.read(&mut new_frame_data[..]) {
            Ok(sz) => {
                new_frame.resize_default(sz)?;
                rx_prod.enqueue(new_frame).map_err(|_e| ())?;
                Ok(sz)
            }
            Err(usb_device::UsbError::WouldBlock) => Ok(0),
            Err(e) => {
                // Maybe the error is recoverable and we should not panic?
                panic!("usb error: {:?}", e);
            }
        }
    }

    #[task(binds=USBCTRL_IRQ, shared = [usb_serial], local=[usb_dev, rx_prod])]
    fn on_usb(ctx: on_usb::Context) {
        let mut usb_serial = ctx.shared.usb_serial;
        let usb_dev = ctx.local.usb_dev;
        let rx_prod = ctx.local.rx_prod;
        usb_serial.lock(|usb_serial| {
            if !usb_dev.poll(&mut [&mut *usb_serial]) {
                return;
            }
            match on_usb_inner(usb_serial, rx_prod) {
                Ok(0) => {}
                Ok(nbytes) => {
                    defmt::trace!("received {} bytes", nbytes);
                }
                Err(_) => {
                    defmt::error!("USB error");
                }
            }
        })
    }
}
