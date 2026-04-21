import zenoh

with zenoh.open(zenoh.Config()) as session:
    session.put("ping", "ping")

