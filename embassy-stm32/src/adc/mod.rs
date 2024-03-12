//! Analog to Digital Converter (ADC)

#![macro_use]
#![allow(missing_docs)] // TODO

#[cfg(any(adc_v1_1, adc_v3))]
mod common;

use cfg_if::cfg_if;
#[cfg(any(adc_v1_1, adc_v3))]
pub use common::*;

#[cfg(not(adc_f3_v2))]
#[cfg_attr(adc_f1, path = "f1.rs")]
#[cfg_attr(adc_f3, path = "f3.rs")]
#[cfg_attr(adc_f3_v1_1, path = "f3_v1_1.rs")]
#[cfg_attr(adc_v1, path = "v1.rs")]
#[cfg_attr(adc_l0, path = "v1.rs")]
#[cfg_attr(adc_v2, path = "v2.rs")]
#[cfg_attr(any(adc_v3, adc_g0, adc_h5), path = "v3.rs")]
#[cfg_attr(adc_v4, path = "v4.rs")]
mod _version;

#[allow(unused)]
#[cfg(not(adc_f3_v2))]
pub use _version::*;

#[cfg(not(any(adc_f1, adc_f3_v2)))]
pub use crate::pac::adc::vals::Res as Resolution;
pub use crate::pac::adc::vals::SampleTime;
use crate::peripherals;

/// Analog to Digital driver.
pub struct Adc<'d, T: Instance> {
    #[allow(unused)]
    adc: crate::PeripheralRef<'d, T>,
    #[cfg(not(any(adc_f3_v2, adc_f3_v1_1, adc_v3)))]
    sample_time: SampleTime,
}

pub(crate) mod sealed {
    use embassy_futures::yield_now;
    #[cfg(any(adc_f1, adc_f3, adc_l0, adc_v1, adc_v3, adc_f3_v1_1))]
    use embassy_sync::waitqueue::AtomicWaker;

    use super::{
        common::{AdcCal, Error, RawValue},
        AdcConfig, PinConfig,
    };

    #[cfg(any(adc_f1, adc_f3, adc_v1, adc_v3, adc_l0, adc_f3_v1_1))]
    pub struct State {
        pub waker: AtomicWaker,
    }

    #[cfg(any(adc_f1, adc_f3, adc_v1, adc_v3, adc_l0, adc_f3_v1_1))]
    impl State {
        pub const fn new() -> Self {
            Self {
                waker: AtomicWaker::new(),
            }
        }
    }

    pub trait InterruptableInstance {
        type Interrupt: crate::interrupt::typelevel::Interrupt;
    }

    pub trait Instance: InterruptableInstance + Sized {
        fn regs() -> crate::pac::adc::Adc;
        #[cfg(not(any(adc_f1, adc_v1, adc_l0, adc_f3_v2, adc_f3_v1_1, adc_g0)))]
        fn common_regs() -> crate::pac::adccommon::AdcCommon;
        #[cfg(any(adc_f1, adc_f3, adc_v1, adc_v3, adc_l0, adc_f3_v1_1))]
        fn state() -> &'static State;
    }

    pub trait AdcImpl: Instance {
        /// VREF voltage used for factory calibration of VREFINTCAL register.
        const VREF_CALIB_UV: u32;

        type Events;

        async fn init();

        /// Returns true if the ADC is awake and ready to perform conversions
        fn is_awake() -> bool;

        /// Returns true if the ADC currently performing conversions
        fn is_running() -> bool;

        async fn wake();
        async fn sleep();

        async fn start_vref();
        fn stop_vref();
        fn vref_factory_cal() -> RawValue;

        fn take_events(interest: Self::Events) -> Self::Events;
        fn clear_events(interest: Self::Events);
        fn set_interest(interest: Self::Events);
        async fn wait_for_events(interest: Self::Events) -> Self::Events;

        async fn set_sequence(channel: &[u8]);

        async fn set_config(config: AdcConfig) -> Result<(), Error>;
        fn get_config() -> AdcConfig;

        async fn set_pin_cfg(pin: u8, cfg: PinConfig) -> Result<(), Error>;
        fn get_pin_cfg(pin: u8) -> PinConfig;

        async fn start_conversions();
        fn stop_conversions();

        /// Reads a single value from the DR when ready
        async fn read_single() -> Result<u16, Error>;
    }

    pub trait AdcPin<T: Instance> {
        #[cfg(any(adc_v1, adc_l0, adc_v2))]
        fn set_as_analog(&mut self) {}

        fn channel(&self) -> u8;
    }

    pub trait InternalChannel<T> {
        fn channel(&self) -> u8;
    }
}

/// ADC instance.
#[cfg(not(any(adc_f1, adc_v1, adc_l0, adc_v2, adc_v3, adc_v4, adc_f3, adc_f3_v1_1, adc_g0, adc_h5)))]
pub trait Instance: sealed::Instance + crate::Peripheral<P = Self> {}
/// ADC instance.
#[cfg(any(adc_f1, adc_v1, adc_l0, adc_v2, adc_v3, adc_v4, adc_f3, adc_f3_v1_1, adc_g0, adc_h5))]
pub trait Instance: sealed::AdcImpl + crate::Peripheral<P = Self> + crate::rcc::RccPeripheral {}

/// ADC pin.
pub trait AdcPin<T: Instance>: sealed::AdcPin<T> {}
/// ADC internal channel.
pub trait InternalChannel<T>: sealed::InternalChannel<T> {}

foreach_adc!(
    ($inst:ident, $common_inst:ident, $clock:ident) => {
        impl crate::adc::sealed::Instance for peripherals::$inst {
            fn regs() -> crate::pac::adc::Adc {
                crate::pac::$inst
            }

            #[cfg(not(any(adc_f1, adc_v1, adc_l0, adc_f3_v2, adc_f3_v1_1, adc_g0)))]
            fn common_regs() -> crate::pac::adccommon::AdcCommon {
                return crate::pac::$common_inst
            }

            #[cfg(any(adc_f1, adc_f3, adc_v1, adc_v3, adc_l0, adc_f3_v1_1))]
            fn state() -> &'static sealed::State {
                static STATE: sealed::State = sealed::State::new();
                &STATE
            }
        }

        foreach_interrupt!(
            ($inst,adc,ADC,GLOBAL,$irq:ident) => {
                impl sealed::InterruptableInstance for peripherals::$inst {
                    type Interrupt = crate::interrupt::typelevel::$irq;
                }
            };
        );

        impl crate::adc::Instance for peripherals::$inst {}
    };
);

macro_rules! impl_adc_pin {
    ($inst:ident, $pin:ident, $ch:expr) => {
        impl crate::adc::AdcPin<peripherals::$inst> for crate::peripherals::$pin {}

        impl crate::adc::sealed::AdcPin<peripherals::$inst> for crate::peripherals::$pin {
            #[cfg(any(adc_v1, adc_l0, adc_v2))]
            fn set_as_analog(&mut self) {
                <Self as crate::gpio::sealed::Pin>::set_as_analog(self);
            }

            fn channel(&self) -> u8 {
                $ch
            }
        }
    };
}
