//! Shared by the interop tests.

use embedded_io_adapters::tokio_1::FromTokio;
use minimeater_app::net::Connector;
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

/// Plain TCP, as embassy-net would give the firmware.
pub struct Tcp;

impl Connector for Tcp {
    type Reader<'a> = FromTokio<OwnedReadHalf>;
    type Writer<'a> = FromTokio<OwnedWriteHalf>;

    async fn connect<'a>(&'a mut self, host: &str, port: u16) -> Result<(Self::Reader<'a>, Self::Writer<'a>), ()> {
        let s = TcpStream::connect((host, port)).await.map_err(|_| ())?;
        let (r, w) = s.into_split();
        Ok((FromTokio::new(r), FromTokio::new(w)))
    }
}

/// A status like a cook in progress.
pub fn status(up_s: u64, sp: f64) -> minimeater_app::tasks::Status {
    use minimeater_app::config::{Cook, Mode};
    use minimeater_app::pit::{Output, Phase};
    use minimeater_app::probes::ProbeSt;
    use minimeater_app::tasks::{AutotuneInfo, Faults, Status};
    use minimeater_core::control::Fire;
    use minimeater_core::food::{Food, Stall};
    use minimeater_core::model::airflow::Actuation;
    let cook = Cook {
        sp_f: sp,
        mode: Mode::Run,
        ..Default::default()
    };
    Status {
        up_s,
        cook,
        out: Output {
            phase: Phase::Hold,
            q: 0.8,
            act: Actuation { damper: 0.3, fan: 0.25 },
            flap_pct: 34.0,
            ref_f: sp,
            lid: false,
            fire: Fire::Ok,
        },
        temps: [Some(sp - 0.4), Some(150.0), None, None],
        probe_st: [ProbeSt::Ok, ProbeSt::Ok, ProbeSt::Open, ProbeSt::Open],
        alarms: [None; 4],
        alarm_led: false,
        adc_die_c: Some(40.0),
        fan_rpm: Some(900.0),
        faults: Faults::default(),
        autotune: AutotuneInfo::default(),
        foods: [
            None,
            Some(Food {
                rate_f_h: Some(2.5),
                eta_s: Some(21600.0),
                stall: Stall::None,
            }),
            None,
            None,
        ],
        ambient_c: Some(20.0),
    }
}
