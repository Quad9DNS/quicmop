use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum QuicmopKernelAgentError {
    #[snafu(display("Parsing config YAML file failed: {}", source))]
    ConfigYamlParsing { source: serde_yaml::Error },
}
