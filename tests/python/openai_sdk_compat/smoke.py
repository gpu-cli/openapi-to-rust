#!/usr/bin/env python3
"""Exercise the generated Axum server through the pinned official OpenAI SDK."""

from __future__ import annotations

import sys

import openai
from openai import OpenAI


EXPECTED_OPENAI_VERSION = "2.45.0"


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} BASE_URL")
    if openai.__version__ != EXPECTED_OPENAI_VERSION:
        raise AssertionError(
            f"expected openai=={EXPECTED_OPENAI_VERSION}, got {openai.__version__}"
        )

    client = OpenAI(
        base_url=sys.argv[1].rstrip("/") + "/",
        api_key="test",
        admin_api_key="test-admin",
        max_retries=0,
    )

    unary = client.responses.create(model="gpt-4.1-mini", input="sdk unary")
    assert unary.id == "resp_demo"
    assert unary.object == "response"
    assert unary.model == "gpt-4.1-mini"

    deltas: list[str] = []
    stream = client.responses.create(
        model="gpt-4.1-mini",
        input="sdk streaming",
        stream=True,
    )
    for event in stream:
        if event.type == "response.output_text.delta":
            deltas.append(event.delta)
    assert "".join(deltas) == "hello world"

    items = client.responses.input_items.list(
        "resp_sdk",
        after="item_1",
        limit=7,
        order="asc",
    )
    assert items.data == []
    # `first_id` is an extra on the SDK's generic cursor page. The demo server
    # echoes the path and query values into it, proving their wire encoding.
    assert items.first_id == "resp_sdk:7:item_1"

    costs = client.admin.organization.usage.costs(
        start_time=1_730_419_200,
        limit=3,
    )
    assert costs.object == "page"
    assert costs.next_page == "start:1730419200"

    print(f"openai-python {openai.__version__} compatibility smoke passed")


if __name__ == "__main__":
    main()
