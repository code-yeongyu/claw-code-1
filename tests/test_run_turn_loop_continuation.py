"""Tests for run_turn_loop continuation contract (ROADMAP #163).

The deprecated ``f'{prompt} [turn N]'`` suffix injection is gone. Verifies:
- No ``[turn N]`` string ever lands in a submitted prompt
- Default (``continuation_prompt=None``) stops the loop after turn 0
- Explicit ``continuation_prompt`` is submitted verbatim on subsequent turns
- The first turn always gets the original prompt, not the continuation
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.models import UsageSummary  # noqa: E402
from src.query_engine import TurnResult  # noqa: E402
from src.runtime import PortRuntime  # noqa: E402


def _completed_result(prompt: str) -> TurnResult:
    return TurnResult(
        prompt=prompt,
        output='ok',
        matched_commands=(),
        matched_tools=(),
        permission_denials=(),
        usage=UsageSummary(),
        stop_reason='completed',
    )


class TestNoTurnSuffixInjection:
    """Core acceptance: no prompt submitted to the engine ever contains '[turn N]'."""

    def test_default_path_submits_original_prompt_only(self) -> None:
        runtime = PortRuntime()
        submitted: list[str] = []

        def _capture(prompt, commands, tools, denials):
            submitted.append(prompt)
            return _completed_result(prompt)

        with patch('src.runtime.QueryEnginePort.from_workspace') as mock_factory:
            engine = mock_factory.return_value
            engine.submit_message.side_effect = _capture

            runtime.run_turn_loop('investigate this bug', max_turns=3)

            # Without continuation_prompt, only turn 0 should run
            assert submitted == ['investigate this bug']
            # And no '[turn N]' suffix anywhere
            for p in submitted:
                assert '[turn' not in p, f'found [turn suffix in submitted prompt: {p!r}'

    def test_with_continuation_prompt_no_turn_suffix(self) -> None:
        runtime = PortRuntime()
        submitted: list[str] = []

        def _capture(prompt, commands, tools, denials):
            submitted.append(prompt)
            return _completed_result(prompt)

        with patch('src.runtime.QueryEnginePort.from_workspace') as mock_factory:
            engine = mock_factory.return_value
            engine.submit_message.side_effect = _capture

            runtime.run_turn_loop(
                'investigate this bug',
                max_turns=3,
                continuation_prompt='Continue.',
            )

            # Turn 0 = original, turns 1-2 = continuation, verbatim
            assert submitted == ['investigate this bug', 'Continue.', 'Continue.']
            # No harness-injected suffix anywhere
            for p in submitted:
                assert '[turn' not in p
                assert not p.endswith(']')


class TestContinuationDefaultStopsAfterTurnZero:
    def test_default_continuation_returns_one_result(self) -> None:
        runtime = PortRuntime()
        with patch('src.runtime.QueryEnginePort.from_workspace') as mock_factory:
            engine = mock_factory.return_value
            engine.submit_message.side_effect = lambda p, *_: _completed_result(p)

            results = runtime.run_turn_loop('x', max_turns=5)
            assert len(results) == 1
            assert results[0].prompt == 'x'

    def test_default_continuation_does_not_call_engine_twice(self) -> None:
        runtime = PortRuntime()
        with patch('src.runtime.QueryEnginePort.from_workspace') as mock_factory:
            engine = mock_factory.return_value
            engine.submit_message.side_effect = lambda p, *_: _completed_result(p)

            runtime.run_turn_loop('x', max_turns=10)
            # Exactly one submit_message call despite max_turns=10
            assert engine.submit_message.call_count == 1


class TestExplicitContinuationBehaviour:
    def test_first_turn_always_uses_original_prompt(self) -> None:
        runtime = PortRuntime()
        captured: list[str] = []

        def _capture(prompt, *_):
            captured.append(prompt)
            return _completed_result(prompt)

        with patch('src.runtime.QueryEnginePort.from_workspace') as mock_factory:
            engine = mock_factory.return_value
            engine.submit_message.side_effect = _capture

            runtime.run_turn_loop(
                'original task', max_turns=2, continuation_prompt='keep going'
            )

            assert captured[0] == 'original task'
            assert captured[1] == 'keep going'

    def test_continuation_respects_max_turns(self) -> None:
        runtime = PortRuntime()
        with patch('src.runtime.QueryEnginePort.from_workspace') as mock_factory:
            engine = mock_factory.return_value
            engine.submit_message.side_effect = lambda p, *_: _completed_result(p)

            runtime.run_turn_loop('x', max_turns=3, continuation_prompt='go')
            assert engine.submit_message.call_count == 3


class TestCLIContinuationFlag:
    def test_cli_default_runs_one_turn(self) -> None:
        """Without --continuation-prompt, CLI should emit exactly '## Turn 1'."""
        result = subprocess.run(
            [sys.executable, '-m', 'src.main', 'turn-loop', 'review MCP tool',
             '--max-turns', '3', '--structured-output'],
            check=True, capture_output=True, text=True,
        )
        assert '## Turn 1' in result.stdout
        assert '## Turn 2' not in result.stdout
        assert '[turn' not in result.stdout

    def test_cli_with_continuation_runs_multiple_turns(self) -> None:
        """With --continuation-prompt, CLI should run up to max_turns."""
        result = subprocess.run(
            [sys.executable, '-m', 'src.main', 'turn-loop', 'review MCP tool',
             '--max-turns', '2', '--structured-output',
             '--continuation-prompt', 'continue'],
            check=True, capture_output=True, text=True,
        )
        assert '## Turn 1' in result.stdout
        assert '## Turn 2' in result.stdout
        # The continuation text is visible (it's submitted as the turn prompt)
        # but no harness-injected [turn N] suffix
        assert '[turn' not in result.stdout
