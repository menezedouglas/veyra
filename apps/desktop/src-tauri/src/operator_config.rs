use veyra_daemon::{ConnectionSetup, signaling_endpoint_valid};

pub struct OperatorConfiguration {
    pub endpoint: Option<String>,
    pub setup: ConnectionSetup,
}

impl OperatorConfiguration {
    fn direct() -> Self {
        Self {
            endpoint: Some(String::new()),
            setup: ConnectionSetup {
                configured: true,
                source: "direct",
                reason: Some("Conexão direta sem servidor. Troque o convite e a resposta para conectar; redes com NAT restritivo podem impedir a conexão.".into()),
            },
        }
    }

    pub fn invitation(endpoint: String) -> Self {
        if endpoint.is_empty() {
            return Self::direct();
        }
        Self {
            endpoint: Some(endpoint),
            setup: ConnectionSetup {
                configured: true,
                source: "invitation",
                reason: None,
            },
        }
    }
}

fn invalid(source: &'static str) -> OperatorConfiguration {
    let reason = if source == "manual" {
        "O endereço avançado de conexão é inválido. Use WSS com o caminho /v1/rendezvous e sem credenciais, parâmetros ou fragmentos."
    } else {
        "A configuração de conexão desta versão é inválida. A distribuição do aplicativo precisa ser corrigida."
    };
    OperatorConfiguration {
        endpoint: None,
        setup: ConnectionSetup {
            configured: false,
            source,
            reason: Some(reason.into()),
        },
    }
}

fn configured(endpoint: &str, source: &'static str) -> OperatorConfiguration {
    let endpoint = endpoint.trim();
    if !signaling_endpoint_valid(endpoint) {
        return invalid(source);
    }
    OperatorConfiguration {
        endpoint: Some(endpoint.into()),
        setup: ConnectionSetup {
            configured: true,
            source,
            reason: None,
        },
    }
}

fn resolve_values(
    manual: Option<&str>,
    environment: Option<&str>,
    application: Option<&str>,
) -> OperatorConfiguration {
    if let Some(endpoint) = manual.filter(|value| !value.trim().is_empty()) {
        configured(endpoint, "manual")
    } else if let Some(endpoint) = environment {
        configured(endpoint, "environment")
    } else if let Some(endpoint) = application {
        configured(endpoint, "application")
    } else {
        OperatorConfiguration::direct()
    }
}

pub fn resolve(manual: Option<&str>) -> OperatorConfiguration {
    if manual.is_some_and(|value| !value.trim().is_empty()) {
        return resolve_values(manual, None, None);
    }
    let mut configuration = match std::env::var("VEYRA_SIGNALING_ENDPOINT") {
        Ok(endpoint) => resolve_values(None, Some(&endpoint), None),
        Err(std::env::VarError::NotPresent) => {
            resolve_values(None, None, option_env!("VEYRA_DEFAULT_SIGNALING_ENDPOINT"))
        }
        Err(std::env::VarError::NotUnicode(_)) => invalid("environment"),
    };
    if configuration.setup.source == "environment"
        && configuration
            .endpoint
            .as_deref()
            .is_some_and(|value| value.starts_with("wss://127.0.0.1:"))
        && std::env::var("VEYRA_DEV_LOCAL_OPERATOR_ACTIVE").as_deref() == Ok("1")
    {
        configuration.setup.reason = Some("Serviço local de desenvolvimento. Os convites desta conexão funcionam nesta máquina; uso pela internet exige o serviço público da distribuição.".into());
    }
    configuration
}

#[cfg(test)]
mod tests {
    use super::{OperatorConfiguration, resolve_values};

    const APPLICATION: &str = "wss://application.invalid/v1/rendezvous";
    const ENVIRONMENT: &str = "wss://environment.invalid/v1/rendezvous";
    const MANUAL: &str = "wss://manual.invalid/v1/rendezvous";

    #[test]
    fn uses_distribution_configuration_without_manual_input() {
        let result = resolve_values(None, None, Some(APPLICATION));
        assert_eq!(result.endpoint.as_deref(), Some(APPLICATION));
        assert!(result.setup.configured);
        assert_eq!(result.setup.source, "application");
    }

    #[test]
    fn explicit_selection_and_environment_take_precedence() {
        let result = resolve_values(Some(MANUAL), Some(ENVIRONMENT), Some(APPLICATION));
        assert_eq!(result.endpoint.as_deref(), Some(MANUAL));
        assert_eq!(result.setup.source, "manual");
        let result = resolve_values(Some("  "), Some(ENVIRONMENT), Some(APPLICATION));
        assert_eq!(result.endpoint.as_deref(), Some(ENVIRONMENT));
        assert_eq!(result.setup.source, "environment");
    }

    #[test]
    fn malformed_configurations_never_silently_select_a_fallback() {
        for manual in [
            "https://manual.invalid/v1/rendezvous",
            "wss://manual.invalid/other",
        ] {
            let result = resolve_values(Some(manual), Some(ENVIRONMENT), Some(APPLICATION));
            assert!(result.endpoint.is_none());
            assert!(!result.setup.configured);
            assert_eq!(result.setup.source, "manual");
            assert!(result.setup.reason.is_some());
        }
        for environment in [
            "",
            "not-a-url",
            "wss://u:p@environment.invalid/v1/rendezvous",
        ] {
            let result = resolve_values(None, Some(environment), Some(APPLICATION));
            assert!(result.endpoint.is_none());
            assert!(!result.setup.configured);
            assert_eq!(result.setup.source, "environment");
        }
    }

    #[test]
    fn default_connection_is_direct_and_requires_no_operator() {
        let result = resolve_values(None, None, None);
        assert_eq!(result.endpoint.as_deref(), Some(""));
        assert!(result.setup.configured);
        assert_eq!(result.setup.source, "direct");
        let reason = result.setup.reason.unwrap();
        assert!(reason.contains("sem servidor"));
        assert!(reason.contains("NAT"));
    }

    #[test]
    fn direct_invitation_preserves_the_serverless_diagnostics() {
        let default = resolve_values(None, None, None);
        let invitation = OperatorConfiguration::invitation(String::new());
        assert_eq!(invitation.endpoint, default.endpoint);
        assert_eq!(invitation.setup.source, "direct");
        assert!(invitation.setup.configured);
        assert_eq!(invitation.setup.reason, default.setup.reason);
        let legacy = OperatorConfiguration::invitation(APPLICATION.into());
        assert_eq!(legacy.endpoint.as_deref(), Some(APPLICATION));
        assert_eq!(legacy.setup.source, "invitation");
    }
}
