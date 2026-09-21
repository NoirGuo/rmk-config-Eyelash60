//! Eyelash60 dongle display boundary.
//!
//! The renderer is `rmk-dongle-display` with its Bongo Cat frames.  This file
//! owns only panel I/O and RMK event mapping; transport, BLE state and display
//! assets stay in their respective libraries.

use dongle_display::display::{BatterySource, DisplayEvent, LinkState, OutputKind, PageMask, Size};
use dongle_display::{DongleDisplay, ModifierStyle};
use embassy_nrf::twim::{self, Twim};
use embassy_time::{Instant, Timer};
use rmk::event::{
    ConnectionStatusChangeEvent, KeyboardEvent, LayerChangeEvent, LedIndicatorEvent, ModifierEvent,
    PeripheralBatteryEvent, PeripheralConnectedEvent, SleepStateEvent, WpmUpdateEvent,
};
use rmk::macros::processor;
use rmk::types::connection::ConnectionType;

const OLED_ADDRESS: u8 = 0x3c;
const WIDTH: usize = 128;
const HEIGHT: u16 = 64;
const PAGES: u8 = (HEIGHT / 8) as u8;
const ALL_PAGES: PageMask = PageMask::from_bits((1_u32 << PAGES) - 1);

/// SH1106 in page-addressing mode.
///
/// The common 128-column SH1106 glass is backed by 132 controller columns, so
/// the visible image begins at column two.  This board has no documented
/// electrical inversion, therefore pixels are written directly and A6 is used
/// exactly once at the controller boundary.
pub struct Sh1106<'d> {
    bus: Twim<'d>,
}

impl<'d> Sh1106<'d> {
    pub fn new(bus: Twim<'d>) -> Self {
        Self { bus }
    }

    async fn init(&mut self) -> Result<(), twim::Error> {
        const COMMANDS: &[u8] = &[
            0x00, // command stream
            0xae, // display off
            0xd5, 0x80, // clock divide / oscillator
            0xa8, 0x3f, // 1:64 multiplex
            0xd3, 0x00, // display offset
            0x40, // start line 0
            0xad, 0x8b, // internal DC/DC on
            0xa1, // segment remap
            0xc8, // reversed COM scan
            0xda, 0x12, // 128x64 COM pins
            0x81, 0x8f, // contrast
            0xd9, 0x22, // precharge
            0xdb, 0x40, // VCOMH
            0xa4, // RAM display
            0xa6, // normal (non-inverted) pixels
            0xaf, // display on
        ];
        self.bus.write(OLED_ADDRESS, COMMANDS).await
    }

    async fn clear(&mut self) -> Result<(), twim::Error> {
        let blank = [0_u8; WIDTH];
        for page in 0..PAGES {
            self.write_page(page, &blank).await?;
        }
        Ok(())
    }

    async fn present(
        &mut self,
        framebuffer: &[u8],
        dirty_pages: PageMask,
    ) -> Result<(), twim::Error> {
        for page in 0..PAGES {
            if dirty_pages.contains(page) {
                let start = usize::from(page) * WIDTH;
                self.write_page(page, &framebuffer[start..start + WIDTH])
                    .await?;
            }
        }
        Ok(())
    }

    async fn write_page(&mut self, page: u8, pixels: &[u8]) -> Result<(), twim::Error> {
        self.bus
            .write(OLED_ADDRESS, &[0x00, 0xb0 | page, 0x02, 0x10])
            .await?;
        let mut payload = [0_u8; WIDTH + 1];
        payload[0] = 0x40;
        payload[1..].copy_from_slice(pixels);
        self.bus.write(OLED_ADDRESS, &payload).await
    }
}

#[processor(
    subscribe = [
        KeyboardEvent,
        LayerChangeEvent,
        WpmUpdateEvent,
        LedIndicatorEvent,
        ModifierEvent,
        SleepStateEvent,
        ConnectionStatusChangeEvent,
        PeripheralConnectedEvent,
        PeripheralBatteryEvent
    ],
    poll_interval = 10
)]
pub struct BongoDisplay<'d> {
    oled: Sh1106<'d>,
    renderer: DongleDisplay,
    peripheral_connected: bool,
    ble_connected: bool,
    initialized: bool,
    force_full: bool,
    animation_trigger: u16,
}

impl<'d> BongoDisplay<'d> {
    pub fn new(oled: Sh1106<'d>) -> Self {
        let mut renderer = DongleDisplay::new_with_modifier_style(
            Size::new(WIDTH as u16, HEIGHT),
            ModifierStyle::Pc,
        )
        .expect("128x64 is supported by rmk-dongle-display");
        renderer.apply(DisplayEvent::OutputChanged {
            output: OutputKind::Usb,
        });
        renderer.apply(DisplayEvent::ConnectionChanged {
            state: LinkState::Searching,
        });
        Self {
            oled,
            renderer,
            peripheral_connected: false,
            ble_connected: false,
            initialized: false,
            force_full: true,
            animation_trigger: 0,
        }
    }

    async fn poll(&mut self) {
        if !self.initialized {
            match self.oled.init().await {
                Ok(()) if self.oled.clear().await.is_ok() => {
                    self.initialized = true;
                    self.force_full = true;
                }
                _ => {
                    Timer::after_millis(250).await;
                    return;
                }
            }
        }

        let output = if self.ble_connected {
            OutputKind::Ble { profile: 0 }
        } else {
            OutputKind::Usb
        };
        self.renderer.apply(DisplayEvent::OutputChanged { output });

        if let Ok(rendered) = self.renderer.render(Instant::now().as_millis()) {
            let pages = if self.force_full {
                ALL_PAGES
            } else {
                rendered.dirty_pages
            };
            if !pages.is_empty() {
                if self
                    .oled
                    .present(self.renderer.framebuffer().bytes(), pages)
                    .await
                    .is_ok()
                {
                    self.force_full = false;
                } else {
                    // render() advances the renderer's dirty baseline, so an
                    // unsuccessful transport must reinitialize and redraw all
                    // eight pages instead of trusting the next dirty mask.
                    self.initialized = false;
                    self.force_full = true;
                }
            }
        }
    }

    async fn on_keyboard_event(&mut self, event: KeyboardEvent) {
        if event.pressed {
            self.animation_trigger = self.animation_trigger.wrapping_add(1);
            self.renderer.apply(DisplayEvent::AnimationTrigger {
                id: self.animation_trigger,
            });
        }
    }

    async fn on_layer_change_event(&mut self, event: LayerChangeEvent) {
        self.renderer
            .apply(DisplayEvent::LayerChanged { layer: event.0 });
    }

    async fn on_wpm_update_event(&mut self, event: WpmUpdateEvent) {
        self.renderer
            .apply(DisplayEvent::WpmChanged { wpm: event.0 });
    }

    async fn on_led_indicator_event(&mut self, event: LedIndicatorEvent) {
        let indicators = u8::from(event.0.num_lock()) | (u8::from(event.0.caps_lock()) << 1);
        self.renderer
            .apply(DisplayEvent::IndicatorsChanged { indicators });
    }

    async fn on_modifier_event(&mut self, event: ModifierEvent) {
        self.renderer.apply(DisplayEvent::ModifiersChanged {
            modifiers: event.modifier.into_bits(),
        });
    }

    async fn on_sleep_state_event(&mut self, event: SleepStateEvent) {
        self.renderer
            .apply(DisplayEvent::SleepChanged { sleeping: event.0 });
    }

    async fn on_connection_status_change_event(&mut self, event: ConnectionStatusChangeEvent) {
        if self.peripheral_connected {
            return;
        }
        let output = match event.0.decide_active() {
            Some(ConnectionType::Ble) => OutputKind::Ble {
                profile: event.0.ble.profile,
            },
            _ => OutputKind::Usb,
        };
        self.renderer.apply(DisplayEvent::OutputChanged { output });
        if let OutputKind::Ble { profile } = output {
            self.renderer
                .apply(DisplayEvent::ProfileChanged { profile });
        }
    }

    async fn on_peripheral_connected_event(&mut self, event: PeripheralConnectedEvent) {
        if event.id == 0 {
            self.peripheral_connected = event.connected;
            self.ble_connected = event.connected;
            self.renderer.apply(DisplayEvent::ConnectionChanged {
                state: if self.peripheral_connected {
                    LinkState::Connected
                } else {
                    LinkState::Searching
                },
            });
        }
    }

    async fn on_peripheral_battery_event(&mut self, event: PeripheralBatteryEvent) {
        use rmk::types::battery::BatteryStatus;
        match event.state.0 {
            BatteryStatus::Available {
                charge_state,
                level,
            } => {
                if let Some(percent) = level {
                    self.renderer.apply(DisplayEvent::BatteryChanged {
                        source: BatterySource::Peripheral(event.id as u8),
                        percent,
                        charging: charge_state
                            == rmk::types::battery::ChargeState::Charging,
                    });
                }
            }
            BatteryStatus::Unavailable => {
                self.renderer.apply(DisplayEvent::BatteryUnavailable {
                    source: BatterySource::Peripheral(event.id as u8),
                });
            }
        }
    }
}
