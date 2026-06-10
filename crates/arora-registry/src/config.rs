use std::fs::{read_to_string, write};

use reqwest::{Client, Url};
use semio_client::{
    authentication::{access_token, Config, ConfigMutation},
    context::Context,
    mutation::Mutation,
    user::{self, Login},
};

use crate::RegistryError;

/// Manages options to connect to a registry as follows:
/// if a configuration is specified it will be used and updated
/// with the latest connection information.
/// If user name or password are provided, it will override the configuration.
pub async fn check_and_update_config(
    url: &Url,
    config_path: Option<String>,
    user_name: Option<String>,
    password: Option<String>,
) -> Result<String, RegistryError> {
    // Read the configuration file if specified.
    let mut config: Option<Config> = if let Some(config_path) = &config_path {
        let config_str = read_to_string(config_path).map_err(|err| RegistryError::Generic {
            message: format!("failed to read configuration file {}: {}", config_path, err),
        })?;
        Some(
            serde_yaml::from_str(&config_str).map_err(|err| RegistryError::Generic {
                message: format!(
                    "failed to parse configuration file {}: {}",
                    config_path, err
                ),
            })?,
        )
    } else {
        None
    };

    // Set the registry URL, update configuration file if specified.
    if let Some(conf) = config {
        config = Some(
            ConfigMutation {
                url: Mutation::Set(url.to_string()),
                ..Default::default()
            }
            .next(conf),
        );
    }

    // Authentication.
    let mut token = None;

    // User name is provided, update configuration file if specified.
    if let Some(user_name) = &user_name {
        let password = password.clone().unwrap_or("".to_string());
        let login = Login {
            user_name: user_name.to_owned(),
            password,
        };
        let context = Context::new(
            url.to_owned(),
            Client::builder()
                .build()
                .map_err(|err| RegistryError::Generic {
                    message: format!("failed to create HTTP client: {}", err),
                })?,
        );
        let login_result =
            user::login(&context, login)
                .await
                .map_err(|err| RegistryError::RemoteError {
                    message: format!("failed to login as user \"{}\": {}", user_name, err),
                })?;

        token = Some(login_result.access_token.token.to_owned());
        if let Some(conf) = config {
            config = Some(
                ConfigMutation {
                    access: Mutation::Set(login_result.access_token),
                    refresh: if let Some(refresh_token) = login_result.refresh_token {
                        Mutation::Set(refresh_token)
                    } else {
                        Mutation::Unset
                    },
                    user_id: Mutation::Set(login_result.id),
                    ..Default::default()
                }
                .next(conf),
            );
        }
    }

    // Password is provided without user name, we can't authenticate.
    if password.is_some() {
        return Err(RegistryError::Generic {
            message: "password provided without user name".to_string(),
        });
    }

    // If not yet authenticated, try to authenticate with the configuration file.
    if token.is_none() {
        if let Some(conf) = config {
            let (new_token, config_mutation) =
                access_token(&conf)
                    .await
                    .map_err(|err| RegistryError::Generic {
                        message: format!(
                            "error while refreshing authentication token from configuration: {}",
                            err
                        ),
                    })?;
            token = new_token;
            config = Some(config_mutation.next(conf));
        } else {
            return Err(RegistryError::Generic {
                message: "no authentication information provided".to_string(),
            });
        }
    }

    // Update the configuration file.
    if let Some(conf) = config {
        let config_str = serde_yaml::to_string(&conf).map_err(|err| RegistryError::Generic {
            message: format!("failed to serialize updated configuration: {}", err),
        })?;
        if let Some(config_path) = &config_path {
            write(config_path, config_str).map_err(|err| RegistryError::Generic {
                message: format!("failed to rewrite updated configuration: {}", err),
            })?;
        }
    }

    Ok(token.expect("Token still missing after authentication succeeded."))
}
