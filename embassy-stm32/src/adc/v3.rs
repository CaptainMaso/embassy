use core::future::poll_fn;
use core::future::Future;
use core::marker::PhantomData;
use core::task::Poll;

use embassy_futures::yield_now;
use embassy_hal_internal::into_ref;
use embassy_time::Timer;

use crate::adc::{Adc, AdcPin, Instance, Resolution, SampleTime};
use crate::Peripheral;

/// Default VREF voltage used for sample conversion to millivolts.
pub const VREF_DEFAULT_MV: u32 = 3300;
/// VREF voltage used for factory calibration of VREFINTCAL register.
pub const VREF_CALIB_MV: u32 = 3000;

/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

impl<T: Instance> crate::interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        let isr = T::regs().isr().read();
        T::regs().ier().modify(|w| {
            if isr.adrdy() {
                // ADC Ready
                w.set_adrdyie(false);
            }
            if isr.eoc() {
                // End-of-conversion
                w.set_eocie(false);
            }
            if isr.eos() {
                // End-of-sequence
                w.set_eosie(false);
            }
            if isr.eosmp() {
                // End-of-sampling-phase
                w.set_eosie(false);
            }
            if isr.ovr() {
                // Overrun
                w.set_eosie(false);
            }
        });

        T::state().waker.wake();
    }
}

pub struct VrefInt;
impl<T: Instance> AdcPin<T> for VrefInt {}
impl<T: Instance> super::sealed::AdcPin<T> for VrefInt {
    fn channel(&self) -> u8 {
        cfg_if! {
            if #[cfg(adc_g0)] {
                let val = 13;
            } else if #[cfg(adc_h5)] {
                let val = 17;
            } else {
                let val = 0;
            }
        }
        val
    }
}

pub struct Temperature;
impl<T: Instance> AdcPin<T> for Temperature {}
impl<T: Instance> super::sealed::AdcPin<T> for Temperature {
    fn channel(&self) -> u8 {
        cfg_if! {
            if #[cfg(adc_g0)] {
                let val = 12;
            } else if #[cfg(adc_h5)] {
                let val = 16;
            } else {
                let val = 17;
            }
        }
        val
    }
}

pub struct Vbat;
impl<T: Instance> AdcPin<T> for Vbat {}
impl<T: Instance> super::sealed::AdcPin<T> for Vbat {
    fn channel(&self) -> u8 {
        cfg_if! {
            if #[cfg(adc_g0)] {
                let val = 14;
            } else if #[cfg(adc_h5)] {
                let val = 2;
            } else {
                let val = 18;
            }
        }
        val
    }
}

cfg_if! {
    if #[cfg(adc_h5)] {
        pub struct VddCore;
        impl<T: Instance> AdcPin<T> for VddCore {}
        impl<T: Instance> super::sealed::AdcPin<T> for VddCore {
            fn channel(&self) -> u8 {
                6
            }
        }
    }
}

impl<'d, T: Instance> Adc<'d, T> {
    pub async fn new(adc: impl Peripheral<P = T> + 'd) -> Self {
        into_ref!(adc);
        T::enable_and_reset();
        T::regs().cr().modify(|reg| {
            #[cfg(not(adc_g0))]
            reg.set_deeppwd(false);
            reg.set_advregen(true);
        });

        #[cfg(adc_g0)]
        T::regs().cfgr1().modify(|reg| {
            reg.set_chselrmod(false);
        });

        Timer::after_micros(20).await;

        T::regs().cr().modify(|reg| {
            reg.set_adcal(true);
        });

        while T::regs().cr().read().adcal() {
            yield_now().await;
        }

        Self { adc }
    }

    fn take_events(&mut self, interest: Events) -> Events {
        T::regs().isr().modify(|w| {
            let all_events = Events::from_bits_truncate(w.0);
            let events = all_events.intersection(interest);
            w.0 = events.bits(); // Only clear events that we're interested in and have consumed
            events
        })
    }

    fn clear_events(&mut self, interest: Events) {
        T::regs().isr().write(|w| w.0 = interest.bits());
    }

    fn set_interest(&mut self, interest: Events) {
        T::regs().ier().write(|w| w.0 = interest.bits());
    }

    fn wait_for_events(&mut self, interest: Events) -> impl Future<Output = Events> + crate::Captures<&'d ()> + '_ {
        poll_fn(move |cx| {
            let events = self.take_events(interest);
            if events.is_empty() {
                T::state().waker.register(cx.waker());
                self.set_interest(interest);
                Poll::Pending
            } else {
                self.set_interest(Events::empty());
                Poll::Ready(events)
            }
        })
    }

    pub async fn enable_vrefint(&self) -> VrefInt {
        #[cfg(not(adc_g0))]
        T::common_regs().ccr().modify(|reg| {
            reg.set_vrefen(true);
        });
        #[cfg(adc_g0)]
        T::regs().ccr().modify(|reg| {
            reg.set_vrefen(true);
        });

        // "Table 24. Embedded internal voltage reference" states that it takes a maximum of 12 us
        // to stabilize the internal voltage reference, we wait a little more.
        // TODO: delay 15us
        //cortex_m::asm::delay(20_000_000);
        //delay.delay_us(15);
        Timer::after_micros(15).await;

        VrefInt {}
    }

    pub fn enable_temperature(&self) -> Temperature {
        cfg_if! {
            if #[cfg(adc_g0)] {
                T::regs().ccr().modify(|reg| {
                    reg.set_tsen(true);
                });
            } else if #[cfg(adc_h5)] {
                T::common_regs().ccr().modify(|reg| {
                    reg.set_tsen(true);
                });
            } else {
                T::common_regs().ccr().modify(|reg| {
                    reg.set_ch17sel(true);
                });
            }
        }

        Temperature {}
    }

    pub fn enable_vbat(&self) -> Vbat {
        cfg_if! {
            if #[cfg(adc_g0)] {
                T::regs().ccr().modify(|reg| {
                    reg.set_vbaten(true);
                });
            } else if #[cfg(adc_h5)] {
                T::common_regs().ccr().modify(|reg| {
                    reg.set_vbaten(true);
                });
            } else {
                T::common_regs().ccr().modify(|reg| {
                    reg.set_ch18sel(true);
                });
            }
        }

        Vbat {}
    }

    pub fn set_resolution(&mut self, resolution: Resolution) {
        #[cfg(not(adc_g0))]
        T::regs().cfgr().modify(|reg| reg.set_res(resolution.into()));
        #[cfg(adc_g0)]
        T::regs().cfgr1().modify(|reg| reg.set_res(resolution.into()));
    }

    /*
    /// Convert a raw sample from the `Temperature` to deg C
    pub fn to_degrees_centigrade(sample: u16) -> f32 {
        (130.0 - 30.0) / (VtempCal130::get().read() as f32 - VtempCal30::get().read() as f32)
            * (sample as f32 - VtempCal30::get().read() as f32)
            + 30.0
    }
     */

    /// Perform a single conversion.
    async fn convert(&mut self) -> u16 {
        let cv_events = Events::EOC;
        self.clear_events(cv_events);

        // Start conversion
        T::regs().cr().modify(|reg| {
            reg.set_adstart(true);
        });

        self.wait_for_events(cv_events).await;

        T::regs().dr().read().0 as u16
    }

    pub async fn read(&mut self, pin: &mut impl AdcPin<T>) -> u16 {
        // Make sure bits are off
        while T::regs().cr().read().addis() {
            yield_now().await;
        }

        // Enable ADC
        T::regs().isr().modify(|reg| {
            reg.set_adrdy(true);
        });

        T::regs().cr().modify(|reg| {
            reg.set_aden(true);
        });

        while !T::regs().isr().read().adrdy() {
            yield_now().await;
        }

        // Select channel
        #[cfg(not(adc_g0))]
        T::regs().sqr1().write(|reg| reg.set_sq(0, pin.channel()));

        #[cfg(adc_g0)]
        T::regs().chselr().write(|reg| reg.set_chsel(1 << pin.channel()));

        // Some models are affected by an erratum:
        // If we perform conversions slower than 1 kHz, the first read ADC value can be
        // corrupted, so we discard it and measure again.
        //
        // STM32L471xx: Section 2.7.3
        // STM32G4: Section 2.7.3
        #[cfg(any(rcc_l4, rcc_g4))]
        let _ = self.convert();

        let val = self.convert().await;

        T::regs().cr().modify(|reg| reg.set_addis(true));

        val
    }

    fn set_channel_sample_time(_ch: u8, sample_time: SampleTime) {
        cfg_if! {
            if #[cfg(adc_g0)] {
                T::regs().smpr().modify(|reg| reg.set_smp1(sample_time.into()));
            } else if #[cfg(adc_h5)] {
                match _ch {
                    0..=9 => T::regs().smpr1().modify(|w| w.set_smp(_ch as usize % 10, sample_time.into())),
                    _ => T::regs().smpr2().modify(|w| w.set_smp(_ch as usize % 10, sample_time.into())),
                }
            } else {
                let sample_time = sample_time.into();
                T::regs()
                    .smpr(_ch as usize / 10)
                    .modify(|reg| reg.set_smp(_ch as usize % 10, sample_time));
            }
        }
    }

    #[cfg(not(adc_g0))]
    fn get_channel_sample_time(ch: u8) -> SampleTime {
        let s = T::regs().smpr(ch as usize / 10).read().smp(ch as usize % 10);
        s.into()
    }
}

bitflags::bitflags! {
    #[derive(Debug,Clone,Copy,PartialEq, Eq)]
    struct Events: u32 {
        const ADRDY = 1;
        const EOSMP = 2;
        const EOC = 3;
        const EOS = 4;
        const JEOC = 5;
        const JEOS = 6;
        const AWD1 = 7;
        const AWD2 = 8;
        const AWD3 = 9;
        const JQOVF = 10;
    }
}
pub enum AdcError {
    Overrun,
}
