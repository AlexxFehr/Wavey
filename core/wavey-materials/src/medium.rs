use serde::{Deserialize, Serialize};

/// Air conditions; determines speed of sound, density and air absorption.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Medium {
    pub temperature_c: f64,
    pub relative_humidity_pct: f64,
    pub pressure_kpa: f64,
}

impl Default for Medium {
    fn default() -> Self {
        Self { temperature_c: 20.0, relative_humidity_pct: 50.0, pressure_kpa: 101.325 }
    }
}

impl Medium {
    pub fn kelvin(&self) -> f64 {
        self.temperature_c + 273.15
    }

    pub fn speed_of_sound(&self) -> f64 {
        331.3 * (1.0 + self.temperature_c / 273.15).sqrt()
    }

    /// Density of dry air (kg/m³).
    pub fn density(&self) -> f64 {
        self.pressure_kpa * 1000.0 / (287.05 * self.kelvin())
    }

    /// Characteristic impedance ρc (Pa·s/m).
    pub fn impedance(&self) -> f64 {
        self.density() * self.speed_of_sound()
    }

    /// Atmospheric absorption in dB/m (ISO 9613-1).
    pub fn air_attenuation_db_per_m(&self, f: f64) -> f64 {
        let t = self.kelvin();
        let t0 = 293.15;
        let t01 = 273.16;
        let pr = 101.325;
        let pa = self.pressure_kpa / pr;
        let c = -6.8346 * (t01 / t).powf(1.261) + 4.6151;
        let psat = 10f64.powf(c);
        let h = self.relative_humidity_pct * psat / pa;
        let fr_o = pa * (24.0 + 4.04e4 * h * (0.02 + h) / (0.391 + h));
        let fr_n = pa * (t / t0).powf(-0.5) * (9.0 + 280.0 * h * (-4.170 * ((t / t0).powf(-1.0 / 3.0) - 1.0)).exp());
        let f2 = f * f;
        8.686
            * f2
            * (1.84e-11 / pa * (t / t0).sqrt()
                + (t / t0).powf(-2.5)
                    * (0.01275 * (-2239.1 / t).exp() / (fr_o + f2 / fr_o)
                        + 0.1068 * (-3352.0 / t).exp() / (fr_n + f2 / fr_n)))
    }

    /// Pressure amplitude factor after travelling `r` meters.
    pub fn air_amplitude(&self, f: f64, r: f64) -> f64 {
        10f64.powf(-self.air_attenuation_db_per_m(f) * r / 20.0)
    }

    /// Intensity decay constant m (1/m), as used in Sabine/Eyring with 4mV.
    pub fn air_intensity_coefficient(&self, f: f64) -> f64 {
        self.air_attenuation_db_per_m(f) / (10.0 * std::f64::consts::LOG10_E)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_of_sound_at_20c() {
        assert!((Medium::default().speed_of_sound() - 343.2).abs() < 0.2);
    }

    #[test]
    fn iso_9613_reference_values() {
        // ISO 9613-2 Table 2 (dB/km) at 70 % RH.
        let table = [
            (20.0, [(500.0, 2.8), (1000.0, 5.0), (2000.0, 9.0), (4000.0, 22.9), (8000.0, 76.6)]),
            (10.0, [(500.0, 1.9), (1000.0, 3.7), (2000.0, 9.7), (4000.0, 32.8), (8000.0, 117.0)]),
        ];
        for (temp, rows) in table {
            let m = Medium { temperature_c: temp, relative_humidity_pct: 70.0, pressure_kpa: 101.325 };
            for (f, dbkm) in rows {
                let got = m.air_attenuation_db_per_m(f) * 1000.0;
                assert!((got - dbkm).abs() / dbkm < 0.05, "{temp} °C {f} Hz: {got:.2} dB/km vs {dbkm}");
            }
        }
    }
}
