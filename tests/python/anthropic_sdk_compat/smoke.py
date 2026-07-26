#!/usr/bin/env python3
"""Exercise the generated Axum server through the pinned official Anthropic SDK."""

from __future__ import annotations

import sys

import anthropic
from anthropic import Anthropic


EXPECTED_ANTHROPIC_VERSION = "0.120.0"


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} BASE_URL")
    if anthropic.__version__ != EXPECTED_ANTHROPIC_VERSION:
        raise AssertionError(
            f"expected anthropic=={EXPECTED_ANTHROPIC_VERSION}, got {anthropic.__version__}"
        )

    client = Anthropic(
        base_url=sys.argv[1].rstrip("/") + "/",
        api_key="test",
        max_retries=0,
        timeout=30.0,
    )
    request = {
        "model": "claude-demo",
        "max_tokens": 16,
        "messages": [{"role": "user", "content": "sdk compatibility"}],
    }

    unary = client.messages.create(**request)
    assert unary.id == "msg_demo"
    assert unary.model == "claude-demo"
    assert unary.role == "assistant"
    assert unary.stop_reason == "end_turn"
    assert unary.content[0].type == "text"
    assert unary.content[0].text == "hello (unary)"

    with client.messages.stream(**request) as stream:
        assert "".join(stream.text_stream) == "hello world"
        final = stream.get_final_message()
    assert final.id == "msg_demo"
    assert final.stop_reason == "end_turn"
    assert final.content[0].type == "text"
    assert final.content[0].text == "hello world"

    print(f"anthropic {anthropic.__version__} compatibility smoke passed")


if __name__ == "__main__":
    main()
