#![no_main]
#![no_std]

mod display;

use rmk::macros::rmk_central;

#[rmk_central]
mod keyboard_central {
    use crate::display::{BongoDisplay, Sh1106};
    use embassy_nrf::twim::{self, Twim};
    use static_cell::StaticCell;

    // The normal TOML display block is deliberately absent, so make the
    // central's I2C0 interrupt visible to RMK's generated Irqs binding here.
    add_interrupt! {
        TWISPI0 => ::embassy_nrf::twim::InterruptHandler<::embassy_nrf::peripherals::TWISPI0>;
    }

    #[register_processor(poll)]
    fn bongo_cat_display() -> BongoDisplay<'static> {
        static I2C_BUFFER: StaticCell<[u8; 256]> = StaticCell::new();

        let mut config = twim::Config::default();
        config.sda_pullup = true;
        config.scl_pullup = true;
        let i2c = Twim::new(
            p.TWISPI0,
            Irqs,
            p.P0_17,
            p.P0_20,
            config,
            I2C_BUFFER.init([0; 256]),
        );

        BongoDisplay::new(Sh1106::new(i2c))
    }
}
