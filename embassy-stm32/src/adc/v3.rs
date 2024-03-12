use crate::interrupt::typelevel::Interrupt;
use core::future::poll_fn;
use core::future::Future;
use core::task::Poll;

use cfg_if::cfg_if;
use embassy_futures::yield_now;
use embassy_hal_internal::drop::OnDrop;
use embassy_time::Timer;
use futures::FutureExt;

use crate::adc::{Instance, Resolution};

use super::common::*;
use super::sealed;
use super::sealed::AdcImpl;

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
                w.set_eosmpie(false);
            }
            if isr.ovr() {
                // Overrun
                w.set_ovrie(false);
            }
        });

        T::state().waker.wake();
    }
}

impl<T: sealed::Instance> AdcImpl for T {
    const VREF_CALIB_UV: u32 = 3_000_000;

    type Events = Events;

    fn vref_factory_cal() -> RawValue {
        let adc_cfg = AdcConfig {
            align: Alignment::RightAlign,
            res: Resolution::BITS12,
            os_mul: OverSamplingMult::X1,
            os_div: OverSamplingDiv::Div1,
        };
        RawValue::from_raw(crate::pac::VREFINTCAL.data().read().value(), false, adc_cfg)
    }

    async fn init() {
        T::regs().cr().modify(|reg| {
            #[cfg(not(adc_g0))]
            reg.set_deeppwd(false);
        });

        T::Interrupt::unpend();
        unsafe {
            T::Interrupt::enable();
        }
    }

    fn is_awake() -> bool {
        let cr = T::regs().cr().read();
        cr.advregen() && cr.aden() && !cr.addis()
    }

    fn is_running() -> bool {
        let cr = T::regs().cr().read();
        cr.adstart() && !cr.adstp()
    }

    async fn wake() {
        let disabling = T::regs().cr().read().addis();
        if disabling {
            while T::regs().cr().read().addis() {
                Timer::after_micros(50).await;
            }
        }

        let regen_stopped = !T::regs().cr().read().advregen();
        if regen_stopped {
            trace!("ADC start regen");
            T::regs().cr().modify(|reg| {
                reg.set_advregen(true);
            });

            Timer::after_micros(20).await;
        }

        #[cfg(adc_g0)]
        T::regs().cfgr1().modify(|reg| {
            reg.set_chselrmod(false);
        });

        if disabling || regen_stopped || !T::regs().cr().read().aden() {
            trace!("ADC internal cal");

            T::regs().cr().modify(|reg| {
                reg.set_adcal(true);
            });

            trace!("ADC Waiting for cal finished");

            while T::regs().cr().read().adcal() {
                Timer::after_micros(20).await;
            }

            Timer::after_micros(20).await;

            trace!("ADC start enable");
            Self::clear_events(Events::ADRDY);

            T::regs().cr().modify(|w| w.set_aden(true));

            Self::wait_for_events(Events::ADRDY).await;

            trace!("ADC Woken");
        }
    }

    async fn sleep() {
        Self::stop_conversions();

        while T::regs().cr().read().adstart() {
            yield_now().await;
        }

        T::regs().cr().modify(|reg| reg.set_addis(true));

        while T::regs().cr().read().aden() {
            yield_now().await;
        }

        Timer::after_micros(20).await;

        T::regs().cr().modify(|reg| {
            reg.set_advregen(false);
        });

        while T::regs().cr().read().aden() {
            yield_now().await;
        }
    }

    async fn start_vref() {
        cfg_if!(
            if #[cfg(adc_g0)] {
                let ccr = T::regs().ccr();
            }
            else {
                let ccr = T::common_regs().ccr();
            }
        );

        if !ccr.read().vrefen() {
            ccr.modify(|reg| {
                reg.set_vrefen(true);
            });

            // "Table 24. Embedded internal voltage reference" states that it takes a maximum of 12 us
            // to stabilize the internal voltage reference, we wait a little more.
            // TODO: delay 15us
            //cortex_m::asm::delay(20_000_000);
            Timer::after_micros(15).await;
        }
    }

    fn stop_vref() {
        cfg_if!(
            if #[cfg(adc_g0)] {
                let ccr = T::regs().ccr();
            }
            else {
                let ccr = T::common_regs().ccr();
            }
        );

        ccr.modify(|reg| {
            reg.set_vrefen(false);
        });
    }

    async fn set_sequence(sequence: &[u8]) {
        assert!(sequence.len() <= 16);
        assert!(sequence.len() > 0);

        Self::stop_conversions();

        while Self::is_running() {
            yield_now().await;
        }

        let mut iter = sequence.iter();
        T::regs().sqr1().modify(|w| w.set_l((sequence.len() - 1) as _));
        // for (idx, ch) in iter.by_ref().take(6).enumerate() {
        //     T::regs().sqr5().modify(|w| w.set_sq(idx, *ch));
        // }
        for (idx, ch) in iter.by_ref().take(4).enumerate() {
            T::regs().sqr1().modify(|w| w.set_sq(idx, *ch));
        }
        for (idx, ch) in iter.by_ref().take(5).enumerate() {
            T::regs().sqr2().modify(|w| w.set_sq(idx, *ch));
        }
        for (idx, ch) in iter.by_ref().take(5).enumerate() {
            T::regs().sqr3().modify(|w| w.set_sq(idx, *ch));
        }
        for (idx, ch) in iter.by_ref().take(2).enumerate() {
            T::regs().sqr4().modify(|w| w.set_sq(idx, *ch));
        }
    }

    async fn start_conversions() {
        T::clear_events(Events::all());

        let cr = T::regs().cr();
        let cfg1 = T::regs().cfgr();

        cfg1.modify(|w| {
            w.set_cont(false);
            w.set_dmaen(false);
            w.set_dmacfg(false);
        });

        cr.modify(|w| w.set_adstart(true));
    }

    async fn read_single() -> Result<u16, Error> {
        cortex_m::asm::delay(2_000);

        let ev = Self::wait_for_events(Events::EOS | Events::OVR).await;

        if ev.contains(Events::OVR) {
            Err(Error::Overrun)
        } else {
            Ok(T::regs().dr().read().regular_data())
        }
    }

    fn stop_conversions() {
        T::regs().cr().modify(|reg| reg.set_adstp(true));
    }

    async fn set_pin_cfg(pin: u8, cfg: PinConfig) -> Result<(), Error> {
        let smpr = pin / 10;
        let smpr_idx = pin % 10;

        T::regs()
            .smpr(smpr as usize)
            .modify(|w| w.set_smp(smpr_idx as usize, cfg.speed.into()));

        Ok(())
    }

    fn get_pin_cfg(pin: u8) -> PinConfig {
        let smpr = pin / 10;
        let smpr_idx = pin % 10;

        let sample_time = T::regs().smpr(smpr as usize).read().smp(smpr_idx as usize).into();

        PinConfig { speed: sample_time }
    }

    async fn set_config(config: AdcConfig) -> Result<(), Error> {
        if config.os_mul.to_bit_shift() > 0 || config.os_div.to_bit_shift() > 0 {
            if matches!(config.align, Alignment::LeftAlign) {
                return Err(Error::InvalidConfiguration(
                    "ADC results must be right aligned if oversampling",
                ));
            }

            let total_bits =
                resolution_to_bits(config.res) + config.os_mul.to_bit_shift() - config.os_div.to_bit_shift();

            if total_bits > 16 {
                return Err(Error::InvalidConfiguration(
                    "Oversampling configuration would truncate data",
                ));
            }

            if config.os_mul.to_bit_shift() == 0 && config.os_div.to_bit_shift() > 0 {
                return Err(Error::InvalidConfiguration(
                    "Oversampling divisor must be 1 if multiplier is 1",
                ));
            }
        }

        T::regs().cfgr().modify(|w| {
            w.set_res(config.res);
            w.set_align(matches!(config.align, Alignment::LeftAlign));
        });

        if config.os_mul.to_bit_shift() > 0 {
            T::regs().cfgr2().modify(|w| {
                w.set_ovsr(config.os_mul.to_bit_shift() - 1);
                w.set_ovss(config.os_div.to_bit_shift());
                w.set_rovse(true);
            });
        } else {
            T::regs().cfgr2().modify(|w| {
                w.set_ovsr(0);
                w.set_ovss(0);
                w.set_rovse(false)
            });
        }

        Ok(())
    }

    fn get_config() -> AdcConfig {
        let cfgr1 = T::regs().cfgr().read();
        let cfgr2 = T::regs().cfgr2().read();

        let os_enabled = cfgr2.rovse();

        let (os_mul, os_div) = if os_enabled {
            (
                OverSamplingMult::from_shift(cfgr2.ovsr() + 1).unwrap(),
                OverSamplingDiv::from_shift(cfgr2.ovss()).unwrap(),
            )
        } else {
            (
                OverSamplingMult::from_shift(0).unwrap(),
                OverSamplingDiv::from_shift(0).unwrap(),
            )
        };

        AdcConfig {
            align: if cfgr1.align() {
                Alignment::LeftAlign
            } else {
                Alignment::RightAlign
            },
            res: cfgr1.res(),
            os_mul,
            os_div,
        }
    }

    fn take_events(interest: Self::Events) -> Self::Events {
        let res = T::regs().isr().modify(|w| {
            let all = Events::from_bits_truncate(w.0);
            let events = all.intersection(interest);
            trace!("Read events: {}, taken events: {}", all, events);
            w.0 = events.bits(); // Only clear events that we're interested in and have consumed
            events
        });
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        res
    }

    fn clear_events(interest: Self::Events) {
        T::regs().isr().modify(|w| {
            let all = Events::from_bits_truncate(w.0);
            let events = all.intersection(interest);
            w.0 = events.bits();
        });
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }

    fn set_interest(interest: Self::Events) {
        T::regs().ier().write(|w| {
            w.0 = interest.bits();
        });
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }

    fn wait_for_events(interest: Self::Events) -> impl Future<Output = Self::Events> {
        let _on_drop = OnDrop::new(|| {
            Self::set_interest(Events::empty());
            trace!("Finished waiting for {}", interest);
        });
        let mut poll_count = 0usize;
        poll_fn(move |cx| {
            poll_count += 1;
            let events = Self::take_events(interest);
            trace!("Got ADC Events after {} loops: {:?}", poll_count, events);
            if events.is_empty() {
                T::state().waker.register(cx.waker());
                //cx.waker().wake_by_ref();
                Self::set_interest(interest);
                Poll::Pending
            } else {
                Poll::Ready(events)
            }
        })
    }
}

//     pub fn enable_temperature(&self) -> Temperature {
//         cfg_if! {
//             if #[cfg(adc_g0)] {
//                 T::regs().ccr().modify(|reg| {
//                     reg.set_tsen(true);
//                 });
//             } else if #[cfg(adc_h5)] {
//                 T::common_regs().ccr().modify(|reg| {
//                     reg.set_tsen(true);
//                 });
//             } else {
//                 T::common_regs().ccr().modify(|reg| {
//                     reg.set_ch17sel(true);
//                 });
//             }
//         }

//         Temperature {}
//     }

//     pub fn enable_vbat(&self) -> Vbat {
//         cfg_if! {
//             if #[cfg(adc_g0)] {
//                 T::regs().ccr().modify(|reg| {
//                     reg.set_vbaten(true);
//                 });
//             } else if #[cfg(adc_h5)] {
//                 T::common_regs().ccr().modify(|reg| {
//                     reg.set_vbaten(true);
//                 });
//             } else {
//                 T::common_regs().ccr().modify(|reg| {
//                     reg.set_ch18sel(true);
//                 });
//             }
//         }

//         Vbat {}
//     }

//     pub fn set_resolution(&mut self, resolution: Resolution) {
//         #[cfg(not(adc_g0))]
//         T::regs().cfgr().modify(|reg| reg.set_res(resolution.into()));
//         #[cfg(adc_g0)]
//         T::regs().cfgr1().modify(|reg| reg.set_res(resolution.into()));
//     }

//     /*
//     /// Convert a raw sample from the `Temperature` to deg C
//     pub fn to_degrees_centigrade(sample: u16) -> f32 {
//         (130.0 - 30.0) / (VtempCal130::get().read() as f32 - VtempCal30::get().read() as f32)
//             * (sample as f32 - VtempCal30::get().read() as f32)
//             + 30.0
//     }
//      */
//     pub async fn read(&mut self, pin: &mut impl AdcPin<T>) -> u16 {
//         // Make sure bits are off
//         while T::regs().cr().read().addis() {
//             yield_now().await;
//         }

//         // Enable ADC
//         T::regs().isr().modify(|reg| {
//             reg.set_adrdy(true);
//         });

//         T::regs().cr().modify(|reg| {
//             reg.set_aden(true);
//         });

//         while !T::regs().isr().read().adrdy() {
//             yield_now().await;
//         }

//         // Select channel
//         #[cfg(not(adc_g0))]
//         T::regs().sqr1().write(|reg| reg.set_sq(0, pin.channel()));

//         #[cfg(adc_g0)]
//         T::regs().chselr().write(|reg| reg.set_chsel(1 << pin.channel()));

//         // Some models are affected by an erratum:
//         // If we perform conversions slower than 1 kHz, the first read ADC value can be
//         // corrupted, so we discard it and measure again.
//         //
//         // STM32L471xx: Section 2.7.3
//         // STM32G4: Section 2.7.3
//         #[cfg(any(rcc_l4, rcc_g4))]
//         let _ = self.convert();

//         let val = self.convert().await;

//         T::regs().cr().modify(|reg| reg.set_addis(true));

//         val
//     }

//     fn set_channel_sample_time(_ch: u8, sample_time: SampleTime) {
//         cfg_if! {
//             if #[cfg(adc_g0)] {
//                 T::regs().smpr().modify(|reg| reg.set_smp1(sample_time.into()));
//             } else if #[cfg(adc_h5)] {
//                 match _ch {
//                     0..=9 => T::regs().smpr1().modify(|w| w.set_smp(_ch as usize % 10, sample_time.into())),
//                     _ => T::regs().smpr2().modify(|w| w.set_smp(_ch as usize % 10, sample_time.into())),
//                 }
//             } else {
//                 let sample_time = sample_time.into();
//                 T::regs()
//                     .smpr(_ch as usize / 10)
//                     .modify(|reg| reg.set_smp(_ch as usize % 10, sample_time));
//             }
//         }
//     }

//     #[cfg(not(adc_g0))]
//     fn get_channel_sample_time(ch: u8) -> SampleTime {
//         let s = T::regs().smpr(ch as usize / 10).read().smp(ch as usize % 10);
//         s.into()
//     }

//     fn update_vref(op: i8) {
//         static VREF_STATUS: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

//         if op > 0 {
//             if VREF_STATUS.fetch_add(1, core::sync::atomic::Ordering::SeqCst) == 0 {
//                 T::regs().ccr().modify(|w| w.set_tsvrefe(true));
//             }
//         } else {
//             if VREF_STATUS.fetch_sub(1, core::sync::atomic::Ordering::SeqCst) == 1 {
//                 T::regs().ccr().modify(|w| w.set_tsvrefe(false));
//             }
//         }
//     }
// }

bitflags::bitflags! {
    #[derive(Debug,Clone,Copy,PartialEq, Eq)]
    pub struct Events: u32 {
        const ADRDY = 1 << 0;
        const EOSMP = 1 << 1;
        const EOC = 1 << 2;
        const EOS = 1 << 3;
        const OVR = 1 << 4;
        const JEOC = 1 << 5;
        const JEOS = 1 << 6;
        const AWD1 = 1 << 7;
        const AWD2 = 1 << 8;
        const AWD3 = 1 << 9;
        const JQOVF = 1 << 10;
    }
}

// impl Events {
//     pub fn to_isr(&self) -> crate::pac::adc::regs::Isr {
//         let mut w = crate::pac::adc::regs::Isr(0);
//         w.set_adrdy(self.contains(Events::ADRDY));
//         w.set_eoc(self.contains(Events::EOC));
//         w.set_eos(self.contains(Events::EOS));
//         w.set_eosmp(self.contains(Events::EOSMP));
//         w.set_jeoc(self.contains(Events::JEOC));
//         w.set_jeos(self.contains(Events::JEOS));
//         w.set_jqovf(self.contains(Events::JQOVF));
//         w.set_ovr(self.contains(Events::OVR));
//         w.set_awd(0, self.contains(Events::AWD1));
//         w.set_awd(1, self.contains(Events::AWD2));
//         w.set_awd(2, self.contains(Events::AWD3));
//         w
//     }

//     pub fn to_ier(&self) -> crate::pac::adc::regs::Ier {
//         let mut w = crate::pac::adc::regs::Ier(0);
//         w.set_adrdyie(self.contains(Events::ADRDY));
//         w.set_eocie(self.contains(Events::EOC));
//         w.set_eosie(self.contains(Events::EOS));
//         w.set_eosmpie(self.contains(Events::EOSMP));
//         w.set_jeocie(self.contains(Events::JEOC));
//         w.set_jeosie(self.contains(Events::JEOS));
//         w.set_jqovfie(self.contains(Events::JQOVF));
//         w.set_ovrie(self.contains(Events::OVR));
//         w.set_awd1ie(self.contains(Events::AWD1));
//         w.set_awd2ie(self.contains(Events::AWD2));
//         w.set_awd3ie(self.contains(Events::AWD3));
//         w
//     }

//     pub fn from_isr(w: crate::pac::adc::regs::Isr) -> Self {
//         let mut ev = Events::empty();
//         ev.set(Events::ADRDY, w.adrdy());
//         ev.set(Events::EOC, w.eoc());
//         ev.set(Events::EOS, w.eos());
//         ev.set(Events::EOSMP, w.eosmp());
//         ev.set(Events::JEOC, w.jeoc());
//         ev.set(Events::JEOS, w.jeos());
//         ev.set(Events::JQOVF, w.jqovf());
//         ev.set(Events::OVR, w.ovr());
//         ev.set(Events::AWD1, w.awd(0));
//         ev.set(Events::AWD2, w.awd(1));
//         ev.set(Events::AWD3, w.awd(2));
//         ev
//     }

//     pub fn from_ier(w: crate::pac::adc::regs::Ier) -> Self {
//         let mut ev = Events::empty();
//         ev.set(Events::ADRDY, w.adrdyie());
//         ev.set(Events::EOC, w.eocie());
//         ev.set(Events::EOS, w.eosie());
//         ev.set(Events::EOSMP, w.eosmpie());
//         ev.set(Events::JEOC, w.jeocie());
//         ev.set(Events::JEOS, w.jeosie());
//         ev.set(Events::JQOVF, w.jqovfie());
//         ev.set(Events::OVR, w.ovrie());
//         ev.set(Events::AWD1, w.awd1ie());
//         ev.set(Events::AWD2, w.awd2ie());
//         ev.set(Events::AWD3, w.awd3ie());
//         ev
//     }
// }

#[cfg(feature = "defmt")]
impl defmt::Format for Events {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "Events(");
        let mut names = self.iter_names();

        if let Some((name, _)) = names.next() {
            defmt::write!(fmt, "{}", name);

            for (name, _) in names {
                defmt::write!(fmt, ",{}", name);
            }

            defmt::write!(fmt, ")");
        } else {
            defmt::write!(fmt, " )");
        }
    }
}

// #[cfg(test)]
// mod test {
//     use bitflags::Flags;

//     use super::*;

//     #[test]
//     pub fn round_trip() {
//         for ev in Events::FLAGS {
//             let name = ev.name();
//             let v = ev.value();
//             let ier = v.to_ier();
//             let isr = v.to_isr();

//             let bits = v.bits();
//             let ier_bits = ier.0;
//             let isr_bits = isr.0;

//             assert_eq!(bits, ier_bits, "Bits for {name} should match IER");
//             assert_eq!(bits, isr_bits, "Bits for {name} should match ISR");
//         }
//     }
// }
