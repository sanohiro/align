#!/usr/bin/env python3
"""Validate agy completion before its response enters a host review log."""

import json
import re
import sys
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON field: {key}")
        result[key] = value
    return result


def validate(events, root):
    require(len(events) >= 2 and all(isinstance(e, dict) for e in events),
            "invalid event stream")
    require(events[0].get("event") == "init" and events[-1].get("event") == "result",
            "expected init first and result last")
    start, result = events[0], events[-1].get("result")
    init = start.get("init")
    require(isinstance(init, dict) and isinstance(result, dict), "invalid envelopes")
    require(init.get("model") == "gemini-3.8-flash-high"
            and init.get("agent") == "align-reviewer"
            and init.get("cwd") == root, "wrong model, agent or workspace")
    require(init.get("permission_mode") == "request-review", "permission bypass")
    conversation = start.get("conversation_id")
    require(isinstance(conversation, str)
            and re.fullmatch(r"[A-Za-z0-9-]+", conversation)
            and result.get("conversation_id") == conversation, "invalid conversation identity")
    require(result.get("status") == "SUCCESS" and type(result.get("num_turns")) is int
            and result["num_turns"] == 1 and result.get("error") in (None, "")
            and result.get("denied_actions", []) == [], "incomplete or resumed provider result")

    # init.tools is the CLI's global registry, not the custom agent's capability
    # list. The native agent restricts tools; the actual trace must agree too.
    for event in events[1:-1]:
        require(event.get("event") == "step_update", "duplicate or unknown event")
        step = event.get("step_update")
        require(isinstance(step, dict) and step.get("conversation_id") == conversation,
                "invalid step identity")
        require(step.get("state") in ("ACTIVE", "DONE"), "failed or unfinished step")
        require(step.get("subagent_info") is None and step.get("error") in (None, ""),
                "delegated or failed inspection")
        if step.get("step_type") == "tool":
            tool = step.get("tool_name")
            require(tool in ("view_file", "grep_search"), "non-inspection tool")
            info = step.get("tool_info", {})
            require(isinstance(info, dict) and info.get("name", tool) == tool
                    and info.get("error") in (None, ""), "failed or mismatched tool")
        else:
            require(step.get("tool_name") is None and step.get("tool_info") is None,
                    "tool outside a tool step")

    response = result.get("response")
    require(isinstance(response, str) and response.strip(), "empty review response")
    lines = response.splitlines()
    markers = [line for line in lines if line.startswith("ALIGN_REVIEW_VERDICT=")]
    require(len(markers) == 1 and markers[0] in (
        "ALIGN_REVIEW_VERDICT=CLEAN", "ALIGN_REVIEW_VERDICT=FINDINGS"),
        "expected exactly one complete verdict")
    require(next(line for line in reversed(lines) if line.strip()) == markers[0],
            "trailing response after verdict")
    fence = None
    for line in lines:
        match = re.match(r"^ {0,3}(`{3,}|~{3,})(.*)$", line)
        if match:
            run, info = match.groups()
            if fence is None:
                if run[0] == "~" or "`" not in info:
                    fence = run
            elif run[0] == fence[0] and len(run) >= len(fence) and not info.strip():
                fence = None
        if line == markers[0]:
            require(fence is None, "verdict inside a code fence")
    require(fence is None, "unfinished code fence")
    return conversation, response


def main():
    if len(sys.argv) != 5:
        raise ValueError("usage: review-agy-result.py EVENTS ROOT RESPONSE CONVERSATION")
    events = [json.loads(line, object_pairs_hook=unique_object)
              for line in Path(sys.argv[1]).read_text().splitlines() if line.strip()]
    conversation, response = validate(events, sys.argv[2])
    Path(sys.argv[3]).write_text(response.rstrip() + "\n")
    Path(sys.argv[4]).write_text(conversation + "\n")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"incomplete agy review: {error}", file=sys.stderr)
        sys.exit(3)
