use crate::blocks::{Block, ConfigBlock, Update};
use crate::config::Config;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::scheduler::Task;
use crate::util::{pseudo_uuid, FormatTemplate};
use crate::widget::{I3BarWidget, Spacing};
use crate::widgets::text::TextWidget;
use crossbeam_channel::Sender;
use serde_derive::Deserialize;
use std::{collections::BTreeMap, collections::HashMap, process::Command, time::Duration};

pub struct Fan {
    text: TextWidget,
    id: String,
    update_interval: Duration,
    format: FormatTemplate,
    chip: Option<String>,
    inputs: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct FanConfig {
    /// Update interval in seconds
    #[serde(
        default = "FanConfig::default_interval",
        deserialize_with = "deserialize_duration"
    )]
    pub interval: Duration,

    /// Format override
    #[serde(default = "FanConfig::default_format")]
    pub format: String,

    /// Chip override
    #[serde(default = "FanConfig::default_chip")]
    pub chip: Option<String>,

    /// Inputs whitelist
    #[serde(default = "FanConfig::default_inputs")]
    pub inputs: Option<Vec<String>>,

    #[serde(default = "FanConfig::default_color_overrides")]
    pub color_overrides: Option<BTreeMap<String, String>>,
}

impl FanConfig {
    fn default_format() -> String {
        "{average}RPM".to_owned()
    }

    fn default_interval() -> Duration {
        Duration::from_secs(15)
    }

    fn default_chip() -> Option<String> {
        None
    }

    fn default_inputs() -> Option<Vec<String>> {
        None
    }

    fn default_color_overrides() -> Option<BTreeMap<String, String>> {
        None
    }
}

impl ConfigBlock for Fan {
    type Config = FanConfig;

    fn new(
        block_config: Self::Config,
        config: Config,
        _tx_update_request: Sender<Task>,
    ) -> Result<Self> {
        let id = pseudo_uuid();

        Ok(Fan {
            update_interval: block_config.interval,
            text: TextWidget::new(config, &id)
                .with_icon("fan")
                .with_spacing(Spacing::Normal),
            id,
            format: FormatTemplate::from_string(&block_config.format)
                .block_error("fan", "Invalid format specified for temperature")?,
            chip: block_config.chip,
            inputs: block_config.inputs,
        })
    }
}

type SensorsOutput = HashMap<String, HashMap<String, serde_json::Value>>;
type InputReadings = HashMap<String, f64>;

impl Block for Fan {
    fn update(&mut self) -> Result<Option<Update>> {
        let mut args = vec!["-j"];
        if let Some(ref chip) = &self.chip {
            args.push(chip);
        }
        let output = Command::new("sensors")
            .args(&args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_else(|e| e.to_string());

        let parsed: SensorsOutput = serde_json::from_str(&output)
            .block_error("temperature", "sensors output is invalid")?;

        let mut fans: Vec<i64> = Vec::new();
        for (_chip, inputs) in parsed {
            for (input_name, input_values) in inputs {
                if let Some(ref whitelist) = self.inputs {
                    if !whitelist.contains(&input_name) {
                        continue;
                    }
                }

                let values_parsed: InputReadings = match serde_json::from_value(input_values) {
                    Ok(values) => values,
                    Err(_) => continue, // probably the "Adapter" key, just ignore.
                };

                for (value_name, value) in values_parsed {
                    if !value_name.starts_with("fan") || !value_name.ends_with("input") {
                        continue;
                    }

                    if (0f64..10000f64).contains(&value) {
                        fans.push(value as i64);
                    } else {
                        // This error is recoverable and therefore should not stop the program
                        eprintln!("Fan ({}) outside of range ([0, 10000])", value);
                    }
                }
            }
        }

        if !fans.is_empty() {
            let max: i64 = *fans
                .iter()
                .max()
                .block_error("temperature", "failed to get max temperature")?;
            let min: i64 = *fans
                .iter()
                .min()
                .block_error("temperature", "failed to get min temperature")?;
            let avg: i64 = (fans.iter().sum::<i64>() as f64 / fans.len() as f64).round() as i64;

            let values = map!("{average}" => avg,
                "{min}" => min,
                "{max}" => max);

            self.text.set_text(self.format.render_static_str(&values)?);
        }

        Ok(Some(self.update_interval.into()))
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }

    fn id(&self) -> &str {
        &self.id
    }
}
