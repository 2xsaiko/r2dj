use dasp::interpolate::linear::Linear;
use dasp::sample::Duplex;
use dasp::signal::interpolate::Converter;
use dasp::{Frame, Signal};

pub struct Tap<S> {
    running: bool,
    signal: S,
}

impl<S> Signal for Tap<S>
where
    S: Signal,
{
    type Frame = S::Frame;

    fn next(&mut self) -> Self::Frame {
        if self.running {
            self.signal.next()
        } else {
            Self::Frame::EQUILIBRIUM
        }
    }

    fn is_exhausted(&self) -> bool {
        self.signal.is_exhausted()
    }
}

impl<S> Tap<S> {
    pub fn into_inner(self) -> S {
        self.signal
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }
}

pub struct Limiter<S> {
    signal: S,
    rate: u32,
}

impl<S, T> Limiter<S>
where
    S: Signal,
    S::Frame: Frame<Sample = T>,
    T: Duplex<f64>,
{
    pub fn resample(mut self, rate: u32) -> Limiter<Converter<S, Linear<S::Frame>>> {
        let s1 = self.signal.next();
        let s2 = self.signal.next();
        Limiter {
            signal: self
                .signal
                .from_hz_to_hz(Linear::new(s1, s2), self.rate as f64, rate as f64),
            rate,
        }
    }
}
