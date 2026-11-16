# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Create the minimal rows the worker demo needs in a cloned database.

Run inside the dev environment (``apps/server/dev/django-env.sh``) with
``PLANE_DB`` pointing at a disposable clone:

    PLANE_DB=t_celery_1 apps/server/dev/django.sh shell < tools/celery/fixture.py

The ids match ``tools/celery/cases.py``: workspace slug ``acme``, user
``0d1e2f30-…`` and project ``aaaaaaaa-…``. ``recent_visited_task`` looks the
workspace up by slug and then inserts a ``user_recent_visits`` row with those
foreign keys.
"""

from __future__ import annotations

import uuid

from plane.db.models import Project, User, Workspace

USER_ID = uuid.UUID("0d1e2f30-4456-4789-9abc-def012345678")
WORKSPACE_ID = uuid.UUID("6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60")
PROJECT_ID = uuid.UUID("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")


def main() -> None:
    user, _ = User.objects.get_or_create(
        id=USER_ID,
        defaults={
            "username": "celery-demo",
            "email": "ada@example.com",
            "display_name": "Ada Lovelace",
            "first_name": "Ada",
            "last_name": "Lovelace",
        },
    )
    workspace, _ = Workspace.objects.get_or_create(
        id=WORKSPACE_ID,
        defaults={
            "name": "Acme",
            "slug": "acme",
            "owner": user,
            "created_by_id": user.id,
        },
    )
    Project.objects.get_or_create(
        id=PROJECT_ID,
        defaults={
            "name": "Apollo",
            "identifier": "APO",
            "workspace": workspace,
            "created_by_id": user.id,
        },
    )
    print(
        "fixture:",
        User.objects.filter(id=USER_ID).count(),
        Workspace.objects.filter(slug="acme").count(),
        Project.objects.filter(id=PROJECT_ID).count(),
    )


main()
