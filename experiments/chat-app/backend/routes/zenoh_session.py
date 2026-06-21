"""
Zenoh session configuration endpoint.
Allows the frontend to connect/disconnect the chat bridge from a DVerse session.
"""
from fastapi import APIRouter, Depends
from pydantic import BaseModel

from ..auth import get_current_user
from ..models.user import User

router = APIRouter()


class ZenohSessionConfig(BaseModel):
    router: str = ""
    namespace: str = ""
    connection_string: str = ""  # base64 {host, port, token} from DVerse bridge tab


@router.get("", summary="Get current Zenoh session info")
def get_zenoh_session(current_user: User = Depends(get_current_user)):
    from ..services.zenoh_bridge import zenoh_bridge
    return zenoh_bridge.get_session_info()


@router.put("", summary="Connect to a DVerse Zenoh session")
def put_zenoh_session(
    body: ZenohSessionConfig,
    current_user: User = Depends(get_current_user),
):
    from ..services.zenoh_bridge import zenoh_bridge
    zenoh_bridge.configure(body.router, body.namespace, body.connection_string)
    return zenoh_bridge.get_session_info()


@router.delete("", summary="Disconnect from Zenoh")
def delete_zenoh_session(current_user: User = Depends(get_current_user)):
    from ..services.zenoh_bridge import zenoh_bridge
    zenoh_bridge.close()
    return {"connected": False, "namespace": ""}


@router.get("/nodes", summary="List online DVerse nodes")
def get_zenoh_nodes(current_user: User = Depends(get_current_user)):
    from ..services.zenoh_bridge import zenoh_bridge
    return zenoh_bridge.get_online_nodes()
