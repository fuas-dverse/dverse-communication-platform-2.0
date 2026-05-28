import os
import logfire


def setup_logfire() -> None:
    """Configure Logfire once at application startup.

    Reads LOGFIRE_TOKEN, LOGFIRE_SERVICE_NAME, LOGFIRE_ENVIRONMENT, and
    LOGFIRE_PYDANTIC_RECORD from the environment.  When LOGFIRE_TOKEN is
    absent the SDK still runs and emits structured output to the console,
    so local development works without a cloud account.

    Scrubbing: the default Logfire patterns already redact fields whose
    names contain 'password', 'secret', 'token', or 'api_key'.  We add
    'password_hash' and 'access_token' as extras so hashed credentials and
    JWT bearer strings are never serialised into log records.
    """
    logfire.configure(
        service_name=os.environ.get("LOGFIRE_SERVICE_NAME", "chatapp-backend"),
        environment=os.environ.get("LOGFIRE_ENVIRONMENT", "development"),
        send_to_logfire="if-token-present",
        scrubbing=logfire.ScrubbingOptions(
            extra_patterns=[r"password_hash", r"access_token"],
        ),
    )

    record_mode = os.environ.get("LOGFIRE_PYDANTIC_RECORD", "all")
    if record_mode not in ("all", "failure", "off"):
        record_mode = "all"
    logfire.instrument_pydantic(record=record_mode)  # type: ignore[arg-type]

    logfire.instrument_httpx()
    logfire.instrument_anthropic()
