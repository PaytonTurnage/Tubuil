use async_std::net::*;
use bench::*;
use std::fs;
use structopt::StructOpt;

use float_ord::FloatOrd;

use nhanh::*;

use serde::{
    ser::{SerializeStruct, Serializer},
    Serialize,
};
use std::collections::HashMap;

fn local_address(port: u16) -> SocketAddr {
    format!("127.0.0.1:{}", port)
        .parse()
        .expect("local address")
}

#[derive(Debug, Clone, Serialize)]
struct NetcodeScenario {
    scenario_name: &'static str,
    #[serde(skip_serializing)]
    transfers: Vec<client::Transfer>,
}

#[derive(Debug, Clone, Serialize)]
struct Scenario {
    #[serde(flatten)]
    netcode_scenario: NetcodeScenario,
    #[serde(flatten)]
    network_config: runner::NetworkConfig,
}

impl Scenario {
    async fn run(&self, port: u16, protocol: Protocol) -> client::Summary {
        let server_address = local_address(port);
        let client_options = client::Options {
            address: server_address,
            protocol,
            transfers: self.netcode_scenario.transfers.clone(),
        };

        let runner_options = runner::Options {
            network_config: self.network_config.clone(),
            client_options,
            start_server: true,
            output: None,
        };

        runner::runner_main(runner_options).await.expect(&format!(
            "running scenario {} against protocol {:?}",
            self.netcode_scenario.scenario_name, protocol
        ))
    }
}

#[derive(Debug)]
struct Comparison {
    scenario: Scenario,
    reports: HashMap<Protocol, client::Summary>,
    least_latent: Protocol,
    least_variant: Protocol,
}

impl Comparison {
    fn from_reports(
        scenario: Scenario,
        reports: HashMap<Protocol, client::Summary>,
    ) -> Self {
        let (least_latent, least_variant) = {
            let (least_latent, _) = reports
                .iter()
                .min_by_key(|(_, report)| FloatOrd(report.mean_ms))
                .unwrap();
            let (least_variant, _) = reports
                .iter()
                .min_by_key(|(_, report)| FloatOrd(report.deviation_ms))
                .unwrap();

            (*least_latent, *least_variant)
        };

        Self {
            scenario,
            reports,
            least_latent,
            least_variant,
        }
    }
}

impl Serialize for Comparison {
    fn serialize<S>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let network_config_fields = 6;
        let report_fields = 2;
        let summary_fields = 2;
        let total_fields = network_config_fields
            + report_fields * self.reports.len()
            + summary_fields;

        let mut state =
            serializer.serialize_struct("Comparison", total_fields)?;

        let cfg = &self.scenario.network_config;

        // Conditions
        state.serialize_field("network_delay_ms", &cfg.delay)?;
        state.serialize_field("network_jitter_ms", &cfg.delay)?;
        state.serialize_field(
            "network_delay_correlation",
            &cfg.delay_correlation,
        )?;
        state.serialize_field("network_random_loss", &cfg.random_loss)?;
        state.serialize_field(
            "network_random_loss_correlation",
            &cfg.random_loss_correlation,
        )?;
        state.serialize_field(
            "network_rate_limit_kbits",
            &cfg.rate_limit_kbps,
        )?;

        // Results
        for (protocol, report) in &self.reports {
            state.serialize_field(
                Box::leak(Box::new(format!(
                    "{:?}_mean_round_trip_ms",
                    protocol
                ))),
                &report.mean_ms,
            )?;
            state.serialize_field(
                Box::leak(Box::new(format!(
                    "{:?}_round_trip_deviation_ms",
                    protocol
                ))),
                &report.deviation_ms,
            )?;
        }

        state.serialize_field("least_latent", &self.least_latent)?;
        state.serialize_field("least_variant", &self.least_variant)?;

        state.end()
    }
}

#[derive(Debug, Clone, Serialize)]
struct Report {
    mean_ping_ms: f64,
    ping_deviation_ms: f64,
}

#[derive(Debug, Default)]
struct SimulationData {
    scenarios:
        HashMap<&'static str, (Comparison, HashMap<Protocol, client::Summary>)>,
}

impl SimulationData {
    fn write_out(self, dir: String) {
        let _ = fs::create_dir(&dir);

        let comparison_writer =
            fs::File::create(format!("{}/comparison.csv", dir))
                .expect("opening comparison output file");
        let mut comparison_writer = csv::Writer::from_writer(comparison_writer);

        for (scenario, (comparison, protocols)) in self.scenarios {
            let _ = fs::create_dir(format!("{}/{}", dir, scenario));

            comparison_writer
                .serialize(comparison)
                .expect("writing comparison to file");

            for (protocol, summary) in protocols {
                let writer = fs::File::create(format!(
                    "{}/{}/{:?}.csv",
                    dir, scenario, protocol
                ))
                .expect("opening file to write protocol report");
                let mut writer = csv::Writer::from_writer(writer);

                summary.trip_reports.into_iter().for_each(|report| {
                    writer
                        .serialize(report)
                        .expect("serializing trip reports to file")
                });
            }
        }
    }
}

const DEFAULT_RETURN_COUNT: Option<usize> = Some(200);

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            netcode_scenario: NetcodeScenario {
                scenario_name: "transfer_0_200B_60Hz-full_bandwith",
                transfers: vec![client::Transfer {
                    stream_id: StreamId(0),
                    size: 200,
                    hertz: 60,
                    return_count: DEFAULT_RETURN_COUNT,
                }],
            },
            network_config: runner::NetworkConfig::default(),
        },
        Scenario {
            netcode_scenario: NetcodeScenario {
                scenario_name:
                    "transfer_0_200B_60Hz-transfer_1_800_240Hz-full_bandwidth",
                transfers: vec![
                    client::Transfer {
                        stream_id: StreamId(0),
                        size: 200,
                        hertz: 60,
                        return_count: DEFAULT_RETURN_COUNT,
                    },
                    client::Transfer {
                        stream_id: StreamId(1),
                        size: 200,
                        hertz: 240,
                        return_count: None,
                    },
                ],
            },
            network_config: runner::NetworkConfig::default(),
        },
        Scenario {
            netcode_scenario: NetcodeScenario {
                scenario_name:
                    "transfer_0_200B_60Hz-transfer_1_800_240Hz-1024kbps",
                transfers: vec![
                    client::Transfer {
                        stream_id: StreamId(0),
                        size: 200,
                        hertz: 60,
                        return_count: DEFAULT_RETURN_COUNT,
                    },
                    client::Transfer {
                        stream_id: StreamId(1),
                        size: 200,
                        hertz: 240,
                        return_count: None,
                    },
                ],
            },
            network_config: runner::NetworkConfig {
                rate_limit_kbps: 1024,
                ..Default::default()
            },
        },
        Scenario {
            netcode_scenario: NetcodeScenario {
                scenario_name: "transfer_0_200B_60Hz-half_bandwidth",
                transfers: vec![client::Transfer {
                    stream_id: StreamId(0),
                    size: 200,
                    hertz: 60,
                    return_count: DEFAULT_RETURN_COUNT,
                }],
            },
            network_config: runner::NetworkConfig {
                rate_limit_kbps: 12,
                ..Default::default()
            },
        },
    ]
}

#[derive(Debug, StructOpt)]
struct Options {
    #[structopt(long, short = "f")]
    scenario_filter: Option<String>,
    /// Name of the directory in which to write CSV results.
    #[structopt(long, short = "o")]
    output: String,
}

#[async_std::main]
async fn main() {
    let options = Options::from_args();

    let scenarios = scenarios();
    let scenarios = scenarios.into_iter().filter(|s| {
        options
            .scenario_filter
            .as_ref()
            .map(|pattern| {
                s.netcode_scenario
                    .scenario_name
                    .matches(pattern.as_str())
                    .next()
                    .is_some()
            })
            .unwrap_or(true)
    });

    let mut simulation_data = SimulationData::default();

    let mut port = 1025;
    for scenario in scenarios {
        let mut reports = HashMap::new();
        for protocol in &ALL_PROTOCOLS {
            let report = scenario.run(port, *protocol).await;
            reports.insert(*protocol, report);
            port += 1;
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        let comparison =
            Comparison::from_reports(scenario.clone(), reports.clone());
        simulation_data.scenarios.insert(
            scenario.netcode_scenario.scenario_name,
            (comparison, reports),
        );
    }

    simulation_data.write_out(options.output);
}
