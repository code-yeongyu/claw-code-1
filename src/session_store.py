from __future__ import annotations

import json
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass(frozen=True)
class StoredSession:
    session_id: str
    messages: tuple[str, ...]
    input_tokens: int
    output_tokens: int


class SessionNotFoundError(KeyError):
    """Raised when a requested session does not exist on disk."""

    def __init__(self, session_id: str) -> None:
        super().__init__(f"Session not found: {session_id}")
        self.session_id = session_id


DEFAULT_SESSION_DIR = Path('.port_sessions')


def save_session(session: StoredSession, directory: Path | None = None) -> Path:
    target_dir = directory or DEFAULT_SESSION_DIR
    target_dir.mkdir(parents=True, exist_ok=True)
    path = target_dir / f'{session.session_id}.json'
    path.write_text(json.dumps(asdict(session), indent=2))
    return path


def load_session(session_id: str, directory: Path | None = None) -> StoredSession:
    target_dir = directory or DEFAULT_SESSION_DIR
    path = target_dir / f'{session_id}.json'
    if not path.exists():
        raise SessionNotFoundError(session_id)
    data = json.loads(path.read_text())
    return StoredSession(
        session_id=data['session_id'],
        messages=tuple(data['messages']),
        input_tokens=data['input_tokens'],
        output_tokens=data['output_tokens'],
    )


def list_sessions(directory: Path | None = None) -> list[str]:
    """Return sorted session ids stored in the target directory."""
    target_dir = directory or DEFAULT_SESSION_DIR
    if not target_dir.exists():
        return []
    return sorted(
        path.stem
        for path in target_dir.glob('*.json')
    )


def session_exists(session_id: str, directory: Path | None = None) -> bool:
    """Return True if the session file exists on disk."""
    target_dir = directory or DEFAULT_SESSION_DIR
    return (target_dir / f'{session_id}.json').exists()


def delete_session(session_id: str, directory: Path | None = None) -> bool:
    """Remove the session file if present. Return True on success, False if absent."""
    target_dir = directory or DEFAULT_SESSION_DIR
    path = target_dir / f'{session_id}.json'
    if path.exists():
        path.unlink()
        return True
    return False
