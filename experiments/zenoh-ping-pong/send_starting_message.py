import os
import zenoh

config_file = os.environ.get("CONFIG_FILE")

if config_file:
    print(f"using zenoh config from file: {config_file}")
    config = zenoh.Config.from_file(config_file)
else:
    print("using default zenoh config")
    config = zenoh.Config()

with zenoh.open(config) as session:
    session.put("ping", "ping")

