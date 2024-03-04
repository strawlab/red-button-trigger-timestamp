use chrono::{DateTime, TimeDelta, Utc};
use nalgebra as na;
use std::collections::VecDeque;

#[derive(Debug)]
pub(crate) struct ClockModelFitError(String);

impl std::fmt::Display for ClockModelFitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "error fitting clock model: {}", self.0)
    }
}

impl std::error::Error for ClockModelFitError {}

pub(crate) fn fit_time_model(
    past_data: &[(f64, f64)],
) -> Result<(f64, f64, f64), ClockModelFitError> {
    use na::{OMatrix, OVector, U2};

    let mut a: Vec<f64> = Vec::with_capacity(past_data.len() * 2);
    let mut b: Vec<f64> = Vec::with_capacity(past_data.len());

    for row in past_data.iter() {
        a.push(row.0);
        a.push(1.0);
        b.push(row.1);
    }
    let a = OMatrix::<f64, na::Dyn, U2>::from_row_slice(&a);
    let b = OVector::<f64, na::Dyn>::from_row_slice(&b);

    let epsilon = 1e-10;
    let results = lstsq::lstsq(&a, &b, epsilon).map_err(|msg| ClockModelFitError(msg.into()))?;

    let gain = results.solution[0];
    let offset = results.solution[1];
    let residuals = results.residuals;

    Ok((gain, offset, residuals))
}

#[test]
fn test_fit_time_model() {
    let epsilon = 1e-12;

    let data = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)];
    let (gain, offset, _residuals) = fit_time_model(&data).unwrap();
    assert!((gain - 1.0).abs() < epsilon);
    assert!((offset - 0.0).abs() < epsilon);

    let data = vec![(0.0, 12.0), (1.0, 22.0), (2.0, 32.0), (3.0, 42.0)];
    let (gain, offset, _residuals) = fit_time_model(&data).unwrap();
    assert!((gain - 10.0).abs() < epsilon);
    assert!((offset - 12.0).abs() < epsilon);
}

struct InnerModel {
    gain: f64,
    offset: f64,
}

impl InnerModel {
    fn from_samples(samples: &VecDeque<(f64, f64)>) -> Self {
        let data: Vec<_> = samples.iter().cloned().collect();
        let (gain, offset, _residuals) = fit_time_model(&data).unwrap();
        InnerModel { gain, offset }
    }
}

pub struct ClockModel {
    epoch: DateTime<Utc>,
    device_epoch: Option<u64>,
    /// maximum round trip time
    max_rtt: TimeDelta,
    samples: VecDeque<(f64, f64)>,
    model: Option<InnerModel>,
}

impl Default for ClockModel {
    fn default() -> Self {
        Self::new(TimeDelta::milliseconds(20))
    }
}

impl ClockModel {
    pub fn new(max_rtt: TimeDelta) -> Self {
        Self {
            epoch: Utc::now(),
            device_epoch: None,
            max_rtt,
            samples: Default::default(),
            model: None,
        }
    }
    pub fn update(&mut self, t0: DateTime<Utc>, t1: DateTime<Utc>, device_timestamp: u64) {
        // First remove potentially giant offset from the epoch.
        let t0 = t0 - self.epoch;
        let t1 = t1 - self.epoch;
        if self.device_epoch.is_none() {
            self.device_epoch = Some(device_timestamp);
        }
        let device_timestamp = device_timestamp - self.device_epoch.as_ref().unwrap();

        // Now the giant offset from the epoch is removed.
        let rtt = t1 - t0;
        if rtt > self.max_rtt {
            tracing::warn!(
                "Ignoring clock measurement with round trip time of {} msecs.",
                rtt.num_milliseconds(),
            );
            return;
        }
        let est_time = t0 + (rtt / 2);
        let est_time_micros = est_time.num_microseconds().unwrap();
        self.samples
            .push_back((est_time_micros as f64, device_timestamp as f64));
        while self.samples.len() > 100 {
            self.samples.pop_front();
        }
        if self.samples.len() >= 10 {
            if self.model.is_none() {
                tracing::info!(
                    "Obtained {} samples. Now capable of estimating clock.",
                    self.samples.len()
                );
            }
            self.model = Some(InnerModel::from_samples(&self.samples));
        }
    }

    pub fn compute_utc(&self, device_timestamp: u64) -> Option<DateTime<Utc>> {
        // First remove potentially giant offset from the epoch.
        let device_timestamp = match &self.device_epoch {
            None => {
                return None;
            }
            Some(device_epoch) => device_timestamp - device_epoch,
        };

        // Now the giant offset from the epoch is removed.
        let model = match self.model.as_ref() {
            None => return None,
            Some(m) => m,
        };

        // Compute the predicted time as a float...
        let est_time_micros = device_timestamp as f64 * model.gain + model.offset;
        // ...and convert back to integer.
        if est_time_micros > i64::MAX as f64 {
            return None;
        }
        if est_time_micros < i64::MIN as f64 {
            return None;
        }
        let est_time_micros = est_time_micros as i64;

        // Add back the offset
        let est_time = self.epoch + TimeDelta::microseconds(est_time_micros);
        Some(est_time)
    }
}
