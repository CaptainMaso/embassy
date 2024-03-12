use core::future::poll_fn;
use core::marker::PhantomData;
use core::task::Poll;

use embassy_futures::yield_now;
use embassy_hal_internal::into_ref;
use embassy_time::Instant;

use super::Resolution;
use crate::adc::{Adc, AdcPin, Instance, SampleTime};
use crate::interrupt::typelevel::Interrupt;
use crate::time::Hertz;
use crate::{interrupt, Peripheral};

use super::common::*;

const ADC_FREQ: Hertz = crate::rcc::HSI_FREQ;

pub const VDDA_CALIB_MV: u32 = 3300;
pub const ADC_MAX: u32 = (1 << 12) - 1;
pub const VREF_INT: u32 = 1230;

impl<T: Instance> interrupt::typelevel::Handler<T::Interrupt> for InterruptHandler<T> {
    unsafe fn on_interrupt() {
        if T::regs().sr().read().eoc() {
            T::regs().cr1().modify(|w| w.set_eocie(false));
        } else {
            return;
        }

        T::state().waker.wake();
    }
}

pub enum AdcPowerMode {
    AlwaysOn,
    DelayOff,
    IdleOff,
    DelayIdleOff,
}

pub enum Prescaler {
    Div1,
    Div2,
    Div3,
    Div4,
}

impl<'d, T: Instance> Adc<'d, T> {
    pub fn new(
        adc: impl Peripheral<P = T> + 'd,
        _irq: impl interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Self {
        into_ref!(adc);

        T::enable_and_reset();

        //let r = T::regs();
        //r.cr2().write(|w| w.set_align(true));

        T::Interrupt::unpend();
        unsafe {
            T::Interrupt::enable();
        }

        Self { adc }
    }

    fn freq() -> Hertz {
        let div = T::regs().ccr().read().adcpre() + 1;
        ADC_FREQ / div as u32
    }

    pub async fn set_resolution(&mut self, res: Resolution) {
        let was_on = Self::is_on();
        if was_on {
            self.stop_adc().await;
        }

        T::regs().cr1().modify(|w| w.set_res(res.into()));

        if was_on {
            self.start_adc().await;
        }
    }

    pub fn resolution(&self) -> Resolution {
        T::regs().cr1().read().res()
    }

    pub fn enable_vref(&self) -> Vref<T> {
        update_vref::<T>(1);

        Vref(core::marker::PhantomData)
    }

    pub fn enable_temperature(&self) -> Temperature<T> {
        T::regs().ccr().modify(|w| w.set_tsvrefe(true));

        Temperature::<T>(core::marker::PhantomData)
    }

    /// Perform a single conversion.
    async fn convert(&mut self) -> u16 {
        let was_on = Self::is_on();

        if !was_on {
            self.start_adc().await;
        }

        self.wait_sample_ready().await;

        T::regs().sr().write(|_| {});
        T::regs().cr1().modify(|w| {
            w.set_eocie(true);
            w.set_scan(false);
        });
        T::regs().cr2().modify(|w| {
            w.set_swstart(true);
            w.set_cont(false);
        }); // swstart cleared by HW

        let res = poll_fn(|cx| {
            T::state().waker.register(cx.waker());

            if T::regs().sr().read().eoc() {
                let res = T::regs().dr().read().rdata();
                Poll::Ready(res)
            } else {
                Poll::Pending
            }
        })
        .await;

        if !was_on {
            self.stop_adc().await;
        }

        res
    }

    #[inline(always)]
    fn is_on() -> bool {
        T::regs().sr().read().adons() || T::regs().cr2().read().adon()
    }

    pub async fn start_adc(&self) {
        //defmt::trace!("Turn ADC on");
        T::regs().cr2().modify(|w| w.set_adon(true));
        //defmt::trace!("Waiting for ADC to turn on");

        let mut t = Instant::now();

        while !T::regs().sr().read().adons() {
            yield_now().await;
            if t.elapsed() > embassy_time::Duration::from_millis(1000) {
                t = Instant::now();
                //defmt::trace!("ADC still not on");
            }
        }

        //defmt::trace!("ADC on");
    }

    pub async fn stop_adc(&self) {
        if T::regs().cr2().read().adon() {
            //defmt::trace!("ADC should be on, wait for it to start");
            while !T::regs().csr().read().adons1() {
                yield_now().await;
            }
        }

        //defmt::trace!("Turn ADC off");

        T::regs().cr2().modify(|w| w.set_adon(false));

        //defmt::trace!("Waiting for ADC to turn off");

        while T::regs().csr().read().adons1() {
            yield_now().await;
        }
    }

    pub async fn read(&mut self, pin: &mut impl AdcPin<T>) -> u16 {
        self.set_sample_sequence(&[pin.channel()]).await;
        self.convert().await
    }

    async fn wait_sample_ready(&self) {
        //trace!("Waiting for sample channel to be ready");
        while T::regs().sr().read().rcnr() {
            yield_now().await;
        }
    }

    pub async fn set_sample_time(&mut self, pin: &mut impl AdcPin<T>, sample_time: SampleTime) {
        if Self::get_channel_sample_time(pin.channel()) != sample_time {
            self.stop_adc().await;
            unsafe {
                Self::set_channel_sample_time(pin.channel(), sample_time);
            }
            self.start_adc().await;
        }
    }

    pub fn get_sample_time(&self, pin: &impl AdcPin<T>) -> SampleTime {
        Self::get_channel_sample_time(pin.channel())
    }

    /// Sets the channel sample time
    ///
    /// ## SAFETY:
    /// - ADON == 0 i.e ADC must not be enabled when this is called.
    unsafe fn set_channel_sample_time(ch: u8, sample_time: SampleTime) {
        let sample_time = sample_time.into();

        match ch {
            0..=9 => T::regs().smpr3().modify(|reg| reg.set_smp(ch as _, sample_time)),
            10..=19 => T::regs()
                .smpr2()
                .modify(|reg| reg.set_smp(ch as usize - 10, sample_time)),
            20..=29 => T::regs()
                .smpr1()
                .modify(|reg| reg.set_smp(ch as usize - 20, sample_time)),
            30..=31 => T::regs()
                .smpr0()
                .modify(|reg| reg.set_smp(ch as usize - 30, sample_time)),
            _ => panic!("Invalid channel to sample"),
        }
    }

    fn get_channel_sample_time(ch: u8) -> SampleTime {
        match ch {
            0..=9 => T::regs().smpr3().read().smp(ch as _),
            10..=19 => T::regs().smpr2().read().smp(ch as usize - 10),
            20..=29 => T::regs().smpr1().read().smp(ch as usize - 20),
            30..=31 => T::regs().smpr0().read().smp(ch as usize - 30),
            _ => panic!("Invalid channel to sample"),
        }
        .into()
    }

    /// Sets the sequence to sample the ADC. Must be less than 28 elements.
    async fn set_sample_sequence(&self, sequence: &[u8]) {
        assert!(sequence.len() <= 28);
        let mut iter = sequence.iter();
        T::regs().sqr1().modify(|w| w.set_l((sequence.len() - 1) as _));
        for (idx, ch) in iter.by_ref().take(6).enumerate() {
            T::regs().sqr5().modify(|w| w.set_sq(idx, *ch));
        }
        for (idx, ch) in iter.by_ref().take(6).enumerate() {
            T::regs().sqr4().modify(|w| w.set_sq(idx, *ch));
        }
        for (idx, ch) in iter.by_ref().take(6).enumerate() {
            T::regs().sqr3().modify(|w| w.set_sq(idx, *ch));
        }
        for (idx, ch) in iter.by_ref().take(6).enumerate() {
            T::regs().sqr2().modify(|w| w.set_sq(idx, *ch));
        }
        for (idx, ch) in iter.by_ref().take(4).enumerate() {
            T::regs().sqr1().modify(|w| w.set_sq(idx, *ch));
        }
    }

    fn get_res_clks(res: Resolution) -> u32 {
        match res {
            Resolution::BITS12 => 12,
            Resolution::BITS10 => 11,
            Resolution::BITS8 => 9,
            Resolution::BITS6 => 7,
        }
    }

    fn get_sample_time_clks(sample_time: SampleTime) -> u32 {
        match sample_time {
            SampleTime::CYCLES4 => 4,
            SampleTime::CYCLES9 => 9,
            SampleTime::CYCLES16 => 16,
            SampleTime::CYCLES24 => 24,
            SampleTime::CYCLES48 => 48,
            SampleTime::CYCLES96 => 96,
            SampleTime::CYCLES192 => 192,
            SampleTime::CYCLES384 => 384,
        }
    }

    pub fn sample_time_for_us(&self, us: u32) -> SampleTime {
        let res_clks = Self::get_res_clks(self.resolution());
        let us_clks = us * Self::freq().0 / 1_000_000;
        let clks = us_clks.saturating_sub(res_clks);
        match clks {
            0..=4 => SampleTime::CYCLES4,
            5..=9 => SampleTime::CYCLES9,
            10..=16 => SampleTime::CYCLES16,
            17..=24 => SampleTime::CYCLES24,
            25..=48 => SampleTime::CYCLES48,
            49..=96 => SampleTime::CYCLES96,
            97..=192 => SampleTime::CYCLES192,
            193.. => SampleTime::CYCLES384,
        }
    }

    pub fn us_for_cfg(&self, res: Resolution, sample_time: SampleTime) -> u32 {
        let res_clks = Self::get_res_clks(res);
        let sample_clks = Self::get_sample_time_clks(sample_time);
        (res_clks + sample_clks) * 1_000_000 / Self::freq().0
    }
}

impl<'d, T: Instance> Drop for Adc<'d, T> {
    fn drop(&mut self) {
        while !T::regs().sr().read().adons() {}

        T::regs().cr2().modify(|w| w.set_adon(false));

        T::disable();
    }
}
