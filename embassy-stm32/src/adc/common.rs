use core::fmt::Debug;
use core::future::poll_fn;
use core::marker::PhantomData;

use embassy_futures::yield_now;
use embassy_hal_internal::drop::OnDrop;
use embassy_hal_internal::into_ref;

use crate::adc::sealed::AdcPin as _;
use crate::adc::{Adc, AdcPin, Events, Instance};
use crate::Peripheral;

use super::{sealed, Resolution};
use cfg_if::cfg_if;

impl<'d, T: Instance> Adc<'d, T> {
    /// Initialises the ADC
    ///
    /// Does not actually start the ADC, that is either done by later calling `start`,
    /// or it will be automatically started and stopped when measuring.
    pub async fn new(
        adc: impl Peripheral<P = T> + 'd,
        _irq: impl crate::interrupt::typelevel::Binding<T::Interrupt, InterruptHandler<T>> + 'd,
    ) -> Result<Self, Error> {
        into_ref!(adc);
        T::enable_and_reset();

        T::init().await;

        //let _cfg = T::get_config();

        //T::set_config(AdcConfig::default()).await?;

        Ok(Self { adc })
    }

    /// Returns a virtual pin for measuring the internal voltage reference
    pub async fn vref(&self) -> Vref<T> {
        Vref::init().await
    }

    #[inline]
    pub fn vref_factory_calibration(&self) -> RawValue {
        T::vref_factory_cal()
    }

    /// Performs a measurement of the internal voltage reference that can
    /// be used to convert other raw ADC measurements into and exact voltage
    /// reference.
    pub async fn calibrate(&mut self) -> Result<AdcCal<T>, Error> {
        trace!("ADC: starting cal");

        if !T::is_awake() {
            return Err(Error::AdcAsleep);
        }

        let mut vref = self.vref().await;

        let old_cfg = T::get_config();
        let old_pin_cfg = T::get_pin_cfg(vref.channel());

        let cal_config = AdcConfig {
            align: Alignment::RightAlign,
            res: MAX_RESOLUTION,
            os_mul: OverSamplingMult::X256,
            os_div: OverSamplingDiv::Div16,
        };

        let cal_pin_cfg = PinConfig {
            speed: SampleSpeed::Medium,
        };

        let modified_config = old_cfg != cal_config || old_pin_cfg != cal_pin_cfg;

        if modified_config {
            T::set_config(cal_config).await?;
            T::set_pin_cfg(vref.channel(), cal_pin_cfg).await;
        }

        let vref_val = self.read(&mut vref).await;

        if modified_config {
            T::set_config(old_cfg).await?;
            T::set_pin_cfg(vref.channel(), old_pin_cfg).await;
        }

        Ok(AdcCal {
            vdda: vref_val,
            _t: PhantomData,
        })
    }

    /// Wakes the ADC from sleep mode to more quickly perform ADC reads
    pub async fn wake(&mut self) {
        if !T::is_awake() {
            T::wake().await;
        }
    }

    /// Returns the ADC to sleep mode to reduce power consumption.
    ///
    /// The ADC will be automatically woken on an ADC read, but will go back to
    /// sleep after the read is complete unless it was running at the start.
    pub async fn sleep(&mut self) {
        T::sleep().await;
    }

    pub async fn set_pin_config(&mut self, pin: &mut impl AdcPin<T>, config: PinConfig) -> Result<(), Error> {
        if T::is_running() {
            return Err(Error::ConfigAdcRunning);
        }

        trace!("Setting pin config: {}", defmt::Debug2Format(&config));

        T::set_pin_cfg(pin.channel(), config).await?;

        Ok(())
    }

    pub fn get_pin_config(&self, pin: &impl AdcPin<T>) -> PinConfig {
        T::get_pin_cfg(pin.channel())
    }

    pub async fn set_config(&mut self, config: AdcConfig) -> Result<(), Error> {
        if T::is_running() {
            return Err(Error::ConfigAdcRunning);
        }

        trace!("Setting ADC config: {}", defmt::Debug2Format(&config));

        T::set_config(config).await?;

        Ok(())
    }

    pub fn get_config(&self) -> AdcConfig {
        T::get_config()
    }

    pub async fn read(&mut self, pin: &mut impl AdcPin<T>) -> RawValue {
        if T::is_running() {
            trace!("ADC: stopping conversions");

            T::stop_conversions();

            trace!("ADC: waiting until finished running");

            while T::is_running() {
                yield_now().await;
            }
        }

        trace!("ADC: getting configuration");

        let cfg = T::get_config();

        let stop_conv = OnDrop::new(|| T::stop_conversions());

        trace!("ADC: cfg sequence");

        T::set_sequence(&[pin.channel()]).await;

        trace!("ADC: starting conversions");

        T::start_conversions().await;

        trace!("ADC: reading single");

        let value = T::read_single().await.unwrap();

        trace!("ADC: stopping conversions");

        drop(stop_conv);

        let v = RawValue::from_raw(value, false, cfg);

        trace!(
            "ADC: got value {} => {} from conf: {}",
            value,
            v,
            defmt::Debug2Format(&cfg)
        );

        v
    }
}

/// Interrupt handler.
pub struct InterruptHandler<T: Instance> {
    _phantom: PhantomData<T>,
}

static VREF_COUNT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

pub struct AdcCal<T: sealed::Instance> {
    vdda: RawValue,
    _t: PhantomData<T>,
}

impl<T: Instance> core::fmt::Debug for AdcCal<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let uv = self.vdda_uv();
        let cal = self.vref_factory_cal();
        f.debug_struct(core::any::type_name::<Self>())
            .field("Vdda", &uv)
            .field("Vref_raw", &self.vdda.value)
            .field("Vcal", &cal)
            .finish()
    }
}

#[cfg(feature = "defmt")]
impl<T: Instance> defmt::Format for AdcCal<T> {
    fn format(&self, fmt: defmt::Formatter) {
        let uv = self.vdda_uv();
        //let uv = MicroVolts { value: 3_250_000 };
        let (v, dec) = uv.as_parts();
        let dec = dec / 1000;
        let n = core::any::type_name::<T>();
        let n = n.rsplit_once(':').map(|(_, n)| n).unwrap_or(n);

        let v_cal = self.vref_factory_cal();

        defmt::write!(
            fmt,
            "AdcCal<{}>{{ Vdda: {}.{:03} V, Vdda raw: {}, Vcal: {} }}",
            n,
            v,
            dec,
            self.vdda.value,
            v_cal
        )
        //defmt::write!(fmt, "AdcCal<{}>{{ Vdda raw: {},  cal: {} }}", n, self.vdda.value, v_cal)
    }
}

impl<T: Instance> AdcCal<T> {
    /// Returns the factory calibration value
    #[inline]
    fn vref_factory_cal(&self) -> RawValue {
        T::vref_factory_cal()
    }

    /// Returns the measured VddA in microvolts (uV)
    pub fn vdda_uv(&self) -> MicroVolts {
        let cal = self.vref_factory_cal();
        let vdda_raw = self.vdda.value;
        let cal_raw = cal.value;
        //trace!("Cal = {}, vdda_raw = {}, cal_raw = {}", cal, vdda_raw, cal_raw);
        let vdda = unwrap!(
            ((T::VREF_CALIB_UV / 64) as i32).checked_mul(cal_raw as i32),
            "Expected {} * {} to fit in i64",
            T::VREF_CALIB_UV,
            cal_raw
        );
        let vdda = vdda / vdda_raw as i32 * 64;
        //let vdda = unwrap!(vdda.try_into(), "Expected {} to fit in i32", vdda);
        MicroVolts::from_raw(vdda)
    }

    /// Returns a calibrated voltage value as in microvolts (uV)
    pub fn calibrate_value(&self, value: RawValue) -> MicroVolts {
        let vdda = self.vdda_uv();
        let value = value.value as i64 * vdda.value as i64 / i16::MAX as i64;
        MicroVolts::from_raw(value as i32)
    }
}

pub struct Vref<T: Instance>(core::marker::PhantomData<T>);
impl<T: Instance> AdcPin<T> for Vref<T> {}
impl<T: Instance> super::sealed::AdcPin<T> for Vref<T> {
    fn channel(&self) -> u8 {
        cfg_if! {
            if #[cfg(adc_g0)] {
                let val = 13;
            } else if #[cfg(any(adc_h5,adc_v1_1))] {
                let val = 17;
            } else {
                let val = 0;
            }
        }
        val
    }
}

impl<T: Instance> Vref<T> {
    async fn init() -> Self {
        if VREF_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst) == 0 {
            T::start_vref().await;
        }
        Self(PhantomData)
    }

    /// The value that vref would be if vdda was at VREF_CALIB_UV
    #[inline]
    pub fn factory_cal(&self) -> RawValue {
        T::vref_factory_cal()
    }
}

impl<T: Instance> Drop for Vref<T> {
    fn drop(&mut self) {
        if VREF_COUNT.fetch_sub(1, core::sync::atomic::Ordering::SeqCst) == 1 {
            T::stop_vref();
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawValue {
    value: i16,
}

impl core::fmt::Debug for RawValue {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (i, f) = self.as_percentage_parts();
        write!(fmt, "RawValue({i}.{f:03})")
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for RawValue {
    fn format(&self, fmt: defmt::Formatter) {
        let (i, f) = self.as_percentage_parts();
        defmt::write!(fmt, "RawValue({}.{:03} = {})", i, f, self.value)
    }
}

impl RawValue {
    pub const fn from_raw(raw_value: u16, signed: bool, cfg: AdcConfig) -> Self {
        let res_bits = resolution_to_bits(cfg.res);
        let bits = res_bits + cfg.os_mul.to_bit_shift();
        let bits = bits - cfg.os_div.to_bit_shift();

        //debug!("cfg = {}", defmt::Debug2Format(&cfg));

        // let sign_bits = match (cfg.align, cfg.res, cfg.signed) {
        //     (_, _, false) => 0,
        //     (Alignment::RightAlign, _, true) => u16::MAX
        //     (Alignment::LeftAlign, Resolution::BITS6, false) => todo!(),
        // };

        // let rshift = match (cfg.align, cfg.res, cfg.signed) {
        //     (Alignment::RightAlign, _, _) => 0,
        //     (Alignment::LeftAlign, Resolution::BITS6, true) => 1,
        //     (Alignment::LeftAlign, Resolution::BITS6, false) => 2,
        //     (Alignment::LeftAlign, _, true) => 16u8 - res_bits,
        //     (Alignment::LeftAlign, _, false) => 16u8 - res_bits - 1,
        // };

        let max_value = fill_bits(bits);

        //trace!("Max adc value = {} ({} bits)", max_value, bits);
        //trace!("Adc value = {}", raw_value);

        // let value = unwrap!(
        //     (i16::MAX as i32).checked_mul(raw_value as i32),
        //     "Expected ADC to not exceed 2147483648"
        // ) / max_value as i32;

        let value = ((i16::MAX as i32) * raw_value as i32) / max_value as i32;

        //let value = unwrap!(value.try_into(), "Expected value to fit in i16");

        //trace!("Converted value = {}", value);

        Self { value: value as i16 }
    }

    /// Parts are -100 - 100, 0-1000
    fn as_percentage_parts(&self) -> (i8, u16) {
        let v = self.value as i32;
        let decimal = unwrap!(v.checked_mul(50_000), "Expected not to overflow i32::MAX") / i16::MAX as i32;
        let int = decimal / 500;
        let frac = (decimal.abs() % 500) * 2;
        (int as i8, frac as u16)
    }

    pub const fn as_raw(self) -> i16 {
        self.value
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MicroVolts {
    value: i32,
}

impl MicroVolts {
    pub const fn from_raw(raw: i32) -> Self {
        Self { value: raw }
    }

    pub fn as_f32(self) -> f32 {
        self.value as f32 / 1_000_000.0
    }

    /// Parts are range
    pub const fn as_parts(self) -> (i8, u32) {
        let v = self.value / 1_000_000;
        let dec = self.value.abs() % 1_000_000;
        (v as i8, dec as u32)
    }

    pub const fn as_raw(self) -> i32 {
        self.value
    }
}

impl core::fmt::Debug for MicroVolts {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if f.alternate() {
            let (v, dec) = self.as_parts();
            let width = f.width().unwrap_or(3);
            let div = 10u32.pow(6u32.saturating_sub(width as u32));
            let dec = dec / div;
            write!(f, "{v}.{dec:0.width$} V")
        } else {
            write!(f, "{} uV", self.value)
        }
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for MicroVolts {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "{} uV", self.value)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Overrun,
    ConfigAdcRunning,
    AdcAsleep,
    InvalidConfiguration(&'static str),
}

pub struct Temperature<T: Instance>(Vref<T>);
impl<T: Instance> AdcPin<T> for Temperature<T> {}
impl<T: Instance> super::sealed::AdcPin<T> for Temperature<T> {
    fn channel(&self) -> u8 {
        16
    }
}

pub struct Vbat<T: Instance>(Vref<T>);
impl<T: Instance> AdcPin<T> for Vbat<T> {}
impl<T: Instance> super::sealed::AdcPin<T> for Vbat<T> {
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
        pub struct VddCore<T: Instance>(Vref<T>);
        impl<T: Instance> AdcPin<T> for VddCore<T> {}
        impl<T: Instance> super::sealed::AdcPin<T> for VddCore {
            fn channel(&self) -> u8 {
                6
            }
        }
    }
}

struct DebugRes(Resolution);

impl Debug for DebugRes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if f.alternate() {
            match self.0 {
                #[cfg(adc_v4)]
                Resolution::BITS16 => write!(f, "Resolution(Optimised 0..65536)"),
                #[cfg(adc_v4)]
                Resolution::BITS14 => write!(f, "Resolution(0..16384)"),
                #[cfg(adc_v4)]
                Resolution::BITS14V => write!(f, "Resolution(Optimised 0..16384)"),
                #[cfg(adc_v4)]
                Resolution::BITS12V => write!(f, "Resolution(Optimised 0..4096)"),
                Resolution::BITS12 => write!(f, "Resolution(0..4096)"),
                Resolution::BITS10 => write!(f, "Resolution(0..1024)"),
                Resolution::BITS8 => write!(f, "Resolution(0..256)"),
                Resolution::BITS6 => write!(f, "Resolution(0..64)"),
            }
        } else {
            match self.0 {
                #[cfg(adc_v4)]
                Resolution::BITS16 => write!(f, "Resolution(Optimised 16 Bits)"),
                #[cfg(adc_v4)]
                Resolution::BITS14 => write!(f, "Resolution(14 Bits)"),
                #[cfg(adc_v4)]
                Resolution::BITS14V => write!(f, "Resolution(Optimised 14 Bits)"),
                #[cfg(adc_v4)]
                Resolution::BITS12V => write!(f, "Resolution(Optimised 12 Bits)"),
                Resolution::BITS12 => write!(f, "Resolution(12 Bits)"),
                Resolution::BITS10 => write!(f, "Resolution(10 Bits)"),
                Resolution::BITS8 => write!(f, "Resolution(8 Bits)"),
                Resolution::BITS6 => write!(f, "Resolution(6 Bits)"),
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct AdcConfig {
    pub align: Alignment,
    pub res: Resolution,
    pub os_mul: OverSamplingMult,
    pub os_div: OverSamplingDiv,
}

impl Default for AdcConfig {
    fn default() -> Self {
        Self {
            align: Alignment::RightAlign,
            res: Resolution::BITS12,
            os_mul: OverSamplingMult::X1,
            os_div: OverSamplingDiv::Div1,
        }
    }
}

impl core::fmt::Debug for AdcConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct(core::any::type_name::<Self>())
            .field("alignment", &self.align)
            .field("resolution", &DebugRes(self.res))
            .field("oversampling_multiplier", &self.os_mul)
            .field("oversampling_divisor", &self.os_div)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverSamplingMult {
    X1,
    X2,
    X4,
    X8,
    X16,
    X32,
    X64,
    X128,
    X256,
}

impl OverSamplingMult {
    pub const fn from_multiplier(multiplier: u16) -> Self {
        match multiplier {
            0..=1 => Self::X1,
            2 => Self::X2,
            3..=4 => Self::X4,
            5..=8 => Self::X8,
            9..=16 => Self::X16,
            17..=32 => Self::X32,
            33..=64 => Self::X64,
            65..=128 => Self::X128,
            129.. => Self::X256,
        }
    }

    pub const fn from_shift(shift: u8) -> Option<Self> {
        match shift {
            0 => Some(Self::X1),
            1 => Some(Self::X2),
            2 => Some(Self::X4),
            3 => Some(Self::X8),
            4 => Some(Self::X16),
            5 => Some(Self::X32),
            6 => Some(Self::X64),
            7 => Some(Self::X128),
            8 => Some(Self::X256),
            _ => None,
        }
    }

    pub const fn to_multiplier(self) -> u16 {
        1u16 << self.to_bit_shift()
    }

    pub const fn to_bit_shift(self) -> u8 {
        match self {
            Self::X1 => 0,
            Self::X2 => 1,
            Self::X4 => 2,
            Self::X8 => 3,
            Self::X16 => 4,
            Self::X32 => 5,
            Self::X64 => 6,
            Self::X128 => 7,
            Self::X256 => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverSamplingDiv {
    Div1,
    Div2,
    Div4,
    Div8,
    Div16,
    Div32,
    Div64,
    Div128,
    Div256,
}

impl OverSamplingDiv {
    pub const fn from_shift(shift: u8) -> Option<Self> {
        match shift {
            0 => Some(Self::Div1),
            1 => Some(Self::Div2),
            2 => Some(Self::Div4),
            3 => Some(Self::Div8),
            4 => Some(Self::Div16),
            5 => Some(Self::Div32),
            6 => Some(Self::Div64),
            7 => Some(Self::Div128),
            8 => Some(Self::Div256),
            _ => None,
        }
    }

    pub const fn from_divisor(divisor: u16) -> Option<Self> {
        match divisor {
            1 => Some(Self::Div1),
            2 => Some(Self::Div2),
            4 => Some(Self::Div4),
            8 => Some(Self::Div8),
            16 => Some(Self::Div16),
            32 => Some(Self::Div32),
            64 => Some(Self::Div64),
            128 => Some(Self::Div128),
            256 => Some(Self::Div256),
            _ => None,
        }
    }

    pub const fn to_bit_shift(self) -> u8 {
        match self {
            Self::Div1 => 0,
            Self::Div2 => 1,
            Self::Div4 => 2,
            Self::Div8 => 3,
            Self::Div16 => 4,
            Self::Div32 => 5,
            Self::Div64 => 6,
            Self::Div128 => 7,
            Self::Div256 => 8,
        }
    }

    pub const fn to_divisor(self) -> u16 {
        1u16 << self.to_bit_shift()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Alignment {
    RightAlign,
    LeftAlign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SampleSpeed {
    UltraFast,
    SuperFast,
    VeryFast,
    Fast,
    Medium,
    Slow,
    VerySlow,
    SuperSlow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinConfig {
    pub speed: SampleSpeed,
}

impl Default for PinConfig {
    fn default() -> Self {
        Self {
            speed: SampleSpeed::Medium,
        }
    }
}

impl From<stm32_metapac::adc::vals::SampleTime> for SampleSpeed {
    fn from(value: stm32_metapac::adc::vals::SampleTime) -> Self {
        use stm32_metapac::adc::vals::SampleTime;
        match value {
            SampleTime::CYCLES2_5 => Self::UltraFast,
            SampleTime::CYCLES6_5 => Self::SuperFast,
            SampleTime::CYCLES12_5 => Self::VeryFast,
            SampleTime::CYCLES24_5 => Self::Fast,
            SampleTime::CYCLES47_5 => Self::Medium,
            SampleTime::CYCLES92_5 => Self::Slow,
            SampleTime::CYCLES247_5 => Self::VerySlow,
            SampleTime::CYCLES640_5 => Self::SuperSlow,
        }
    }
}

impl From<SampleSpeed> for stm32_metapac::adc::vals::SampleTime {
    fn from(value: SampleSpeed) -> Self {
        use stm32_metapac::adc::vals::SampleTime;
        match value {
            SampleSpeed::UltraFast => SampleTime::CYCLES2_5,
            SampleSpeed::SuperFast => SampleTime::CYCLES6_5,
            SampleSpeed::VeryFast => SampleTime::CYCLES12_5,
            SampleSpeed::Fast => SampleTime::CYCLES24_5,
            SampleSpeed::Medium => SampleTime::CYCLES47_5,
            SampleSpeed::Slow => SampleTime::CYCLES92_5,
            SampleSpeed::VerySlow => SampleTime::CYCLES247_5,
            SampleSpeed::SuperSlow => SampleTime::CYCLES640_5,
        }
    }
}

// #[derive(Clone, Copy)]
// struct CompressedAdcConfig {
//     high: u8,
//     low: u8,
// }

// impl From<AdcConfig> for CompressedAdcConfig {
//     fn from(value: AdcConfig) -> Self {
//         let align = match value.align {
//             Alignment::RightAlign => 0x80,
//             Alignment::LeftAlign => 0x00,
//         };
//         let res = value.res.to_bits();
//         let os_mul = value.os_mul.to_bit_shift();
//         let os_div = value.os_div.to_bit_shift();

//         Self {
//             high: align | res,
//             low: (os_mul << 4) | os_div,
//         }
//     }
// }

// impl From<CompressedAdcConfig> for AdcConfig {
//     fn from(val: CompressedAdcConfig) -> Self {
//         let align = if val.high & 0b1000_0000 > 0 {
//             Alignment::LeftAlign
//         } else {
//             Alignment::RightAlign
//         };

//         let res = Resolution::from_bits(val.high & 0x0F);

//         let os_mul = unsafe { OverSamplingMult::from_shift(val.low >> 4).unwrap_unchecked() };
//         let os_div = unsafe { OverSamplingDiv::from_shift(val.low & 0x0F).unwrap_unchecked() };

//         AdcConfig {
//             align,
//             res,
//             os_mul,
//             os_div,
//         }
//     }
// }

pub const fn fill_bits(bits: u8) -> u16 {
    if let Some(filled) = 2u16.checked_pow(bits as u32) {
        filled - 1
    } else {
        u16::MAX
    }
}

/// Get the bits of resolution for this resolution
///
/// This is `2**n - 1`.
#[cfg(not(any(adc_f1, adc_f3_v2)))]
pub const fn resolution_to_bits(res: Resolution) -> u8 {
    match res {
        #[cfg(adc_v4)]
        Resolution::BITS16 => 16,
        #[cfg(adc_v4)]
        Resolution::BITS14 => 14,
        #[cfg(adc_v4)]
        Resolution::BITS14V => 14,
        #[cfg(adc_v4)]
        Resolution::BITS12V => 12,
        Resolution::BITS12 => 12,
        Resolution::BITS10 => 10,
        Resolution::BITS8 => 8,
        #[cfg(any(adc_v1, adc_v2, adc_v3, adc_l0, adc_g0, adc_f3, adc_f3_v1_1, adc_h5))]
        Resolution::BITS6 => 6,
        #[allow(unreachable_patterns)]
        _ => core::unreachable!(),
    }
}

const MAX_RESOLUTION: Resolution = {
    cfg_if!(
        if #[cfg(adc_v4)] {
            let res = Resolution::BITS16;
        }
        else {
            let res = Resolution::BITS12;
        }
    );
    res
};
