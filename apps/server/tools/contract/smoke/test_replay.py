# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Proof that a Rust-side Celery publish reaches a patched `.delay` mock.

The contract suite asserts on `unittest.mock.patch("…delay")` in a few places.
A Rust handler publishes a real AMQP message instead of calling Python, so the
plugin drains the run's vhost after every Rust request and replays each message
into the patched mock (`pytest_plane_contract._drain_and_replay`). This test is
the mechanism's own proof: it hits a Rust-owned GET that enqueues
`recent_visited_task`, with the task patched, and asserts on the mock's kwargs —
including the `uuid.UUID` that kombu's JSON envelope had to be decoded back
into.

Run it with `run.sh --smoke` (mixed mode only).
"""

import uuid
from unittest.mock import patch

import pytest
from rest_framework.test import APIClient

from plane.db.models import Project, ProjectMember, User, Workspace, WorkspaceMember

import pytest_plane_contract


@pytest.fixture
def workspace_project(db):
    suffix = uuid.uuid4().hex[:8]
    user = User.objects.create(
        email=f"replay-{suffix}@plane.so", username=f"replay-{suffix}", first_name="Replay"
    )
    workspace = Workspace.objects.create(name="Replay", slug=f"replay-{suffix}", owner=user)
    WorkspaceMember.objects.create(workspace=workspace, member=user, role=20)
    project = Project.objects.create(name="Replay Project", identifier="RPY", workspace=workspace)
    ProjectMember.objects.create(workspace=workspace, project=project, member=user, role=20)
    return user, workspace, project


@pytest.mark.django_db
def test_a_rust_publish_reaches_a_patched_delay(workspace_project):
    if pytest_plane_contract.STATE.backend != "mixed":
        pytest.skip("the replay bridge only exists in mixed mode")
    user, workspace, project = workspace_project

    client = APIClient()
    client.force_authenticate(user=user)
    target = "plane.bgtasks.recent_visited_task.recent_visited_task.delay"
    with patch(target) as delay:
        response = client.get(f"/api/workspaces/{workspace.slug}/projects/{project.id}/")

    assert response.status_code == 200, response.content
    assert getattr(response, "was_rust", False), "the request was expected to reach plane-api"
    assert delay.call_count == 1, "the Rust publish did not reach the patched .delay"
    kwargs = delay.call_args.kwargs
    assert kwargs["slug"] == workspace.slug
    assert kwargs["entity_name"] == "project"
    # `project/base.py:247-253` passes the `<uuid:pk>` converter's objects, so
    # both ids are `uuid.UUID` — which is exactly what the kombu envelope has
    # to be decoded back into for `call_args` to match the Python call.
    assert isinstance(kwargs["entity_identifier"], uuid.UUID)
    assert kwargs["entity_identifier"] == project.id
    assert isinstance(kwargs["project_id"], uuid.UUID)
    assert kwargs["project_id"] == project.id
    assert isinstance(kwargs["user_id"], uuid.UUID)
    assert kwargs["user_id"] == user.id
