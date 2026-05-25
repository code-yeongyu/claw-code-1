from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from src.session_store import (
    StoredSession,
    SessionNotFoundError,
    save_session,
    load_session,
    list_sessions,
    session_exists,
    delete_session,
)


class SessionStoreTests(unittest.TestCase):
    def test_list_sessions_returns_sorted_ids(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            save_session(StoredSession('alpha', ('msg1',), 1, 2), directory=root)
            save_session(StoredSession('beta', ('msg2',), 3, 4), directory=root)

            # when
            ids = list_sessions(directory=root)

            # then
            self.assertEqual(ids, ['alpha', 'beta'])

    def test_list_sessions_returns_empty_for_missing_dir(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'nonexistent'

            # when
            ids = list_sessions(directory=root)

            # then
            self.assertEqual(ids, [])

    def test_session_exists_true_when_present(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            save_session(StoredSession('existing', ('msg',), 1, 1), directory=root)

            # when / then
            self.assertTrue(session_exists('existing', directory=root))

    def test_session_exists_false_when_absent(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)

            # when / then
            self.assertFalse(session_exists('missing', directory=root))

    def test_delete_session_removes_file(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            save_session(StoredSession('to-delete', ('msg',), 1, 1), directory=root)
            self.assertTrue(session_exists('to-delete', directory=root))

            # when
            result = delete_session('to-delete', directory=root)

            # then
            self.assertTrue(result)
            self.assertFalse(session_exists('to-delete', directory=root))

    def test_delete_session_returns_false_when_absent(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)

            # when
            result = delete_session('never-existed', directory=root)

            # then
            self.assertFalse(result)

    def test_load_session_raises_typed_error_when_missing(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)

            # when / then
            with self.assertRaises(SessionNotFoundError) as cm:
                load_session('nonexistent', directory=root)
            self.assertEqual(cm.exception.session_id, 'nonexistent')
            self.assertIsInstance(cm.exception, KeyError)

    def test_full_crud_round_trip(self) -> None:
        # given
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            original = StoredSession('round-trip', ('a', 'b'), 10, 20)
            save_session(original, directory=root)

            # when
            self.assertTrue(session_exists('round-trip', directory=root))
            loaded = load_session('round-trip', directory=root)

            # then
            self.assertEqual(loaded.session_id, 'round-trip')
            self.assertEqual(loaded.messages, ('a', 'b'))
            self.assertEqual(loaded.input_tokens, 10)
            self.assertEqual(loaded.output_tokens, 20)

            # cleanup
            delete_session('round-trip', directory=root)
            self.assertFalse(session_exists('round-trip', directory=root))
            self.assertEqual(list_sessions(directory=root), [])


if __name__ == '__main__':
    unittest.main()
