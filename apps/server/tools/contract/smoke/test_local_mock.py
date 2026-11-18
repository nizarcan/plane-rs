# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Proof that `pytest_plane_contract._local_mock_reason` sees in-process patches.

A Rust-owned request is only sent to `plane-api` when no in-process `Mock` would
make the test fail there. This module exercises the detector directly: each
patched target the contract suite replaces must produce a reason that names it
and ends with "served by Django", an unrelated patch on the same module must
not, and the reason must be gone once the patch block exits. It sends no HTTP
request, so it runs with `run.sh --backend django --smoke` too.

The same module covers the settings detector: a value changed through pytest's
`settings` fixture or `override_settings` must fire (the second process read
the original at startup), a value reassigned to an equal one or a setting
outside `ENV_SETTINGS` must stay silent.
"""

from unittest.mock import patch

import pytest
from django.core.cache import cache, caches
from django.test import override_settings
from django.utils.connection import ConnectionProxy

import pytest_plane_contract

SANCTIONED = "served by Django"


def _reason() -> str | None:
    return pytest_plane_contract._local_mock_reason()


def test_nothing_patched_is_rust_eligible():
    assert _reason() is None


@pytest.mark.parametrize(("module_name", "class_name"), pytest_plane_contract.AUTH_PROVIDER_MOCK_TARGETS)
def test_patched_oauth_provider_class_is_a_limitation(module_name, class_name):
    target = f"{module_name}.{class_name}"
    with patch(target) as provider:
        reason = _reason()
        assert reason is not None, target
        assert target in reason
        assert reason.endswith(SANCTIONED)
        provider.assert_not_called()
    assert _reason() is None


def test_another_attribute_of_a_callback_view_is_not_a_limitation():
    """Only the provider class matters: a patched task at the call site is replayable."""
    with patch("plane.authentication.views.app.github.user_login") as login:
        assert _reason() is None
        login.assert_not_called()


@pytest.mark.parametrize("attribute", ["incr", "get", "set", "delete"])
def test_patched_cache_method_is_a_limitation(attribute):
    # The view imports the same `ConnectionProxy` the test patches.
    from plane.app.views.user.base import cache as view_cache

    assert isinstance(cache, ConnectionProxy)
    assert view_cache is cache

    with patch.object(cache, attribute, return_value=1) as mock:
        reason = _reason()
        assert reason is not None, attribute
        assert f"django.core.cache.cache.{attribute}" in reason
        assert reason.endswith(SANCTIONED)
        assert getattr(cache, attribute)("contract-detector-probe") == 1
        mock.assert_called_once_with("contract-detector-probe")
    # `patch` restores the bound method as an instance attribute on the backend,
    # so the proxy is no longer a Mock source for the next test.
    assert _reason() is None
    vars(caches["default"]).pop(attribute, None)


def test_another_mocked_object_is_not_a_cache_limitation():
    """A mock outside the shared cache object must stay Rust-eligible."""
    from plane.authentication.rate_limit import AuthenticationThrottle

    with patch.object(AuthenticationThrottle, "rate", "1/minute"):
        # Detected by AUTH_PATCHED_RATES, not by the cache check, so the reason
        # is the throttle one and the cache check on its own stays quiet.
        assert pytest_plane_contract._cache_mock_reason() is None


def test_settings_fixture_assignment_is_a_limitation(settings):
    """The `settings` fixture is how `test_oauth_callback_state_app.py` does it."""
    original = settings.APP_BASE_URL
    settings.APP_BASE_URL = "http://app.example.com"

    reason = _reason()
    assert reason is not None
    assert "settings.APP_BASE_URL" in reason
    assert "'http://app.example.com'" in reason
    assert f"plane-api read {original!r} at startup" in reason
    assert reason.endswith(SANCTIONED)


def test_override_settings_is_a_limitation():
    with override_settings(APP_BASE_URL="http://app.example.com"):
        reason = _reason()
        assert reason is not None
        assert "settings.APP_BASE_URL" in reason
        assert reason.endswith(SANCTIONED)
    # `override_settings` restores Django's value; the baseline never moved.
    assert _reason() is None


def test_another_environment_setting_is_a_limitation():
    """The detector covers the whole list, not only the setting the suite uses."""
    with override_settings(SESSION_COOKIE_NAME="contract-session"):
        reason = _reason()
        assert reason is not None
        assert "settings.SESSION_COOKIE_NAME" in reason
        assert "contract-session" in reason
        assert reason.endswith(SANCTIONED)


def test_a_list_setting_reassigned_to_an_equal_list_is_not_a_limitation(settings):
    """A fresh-but-equal list must not look like an override.

    `CORS_ALLOWED_ORIGINS` is absent from `settings` in the dev environment
    (Plane only assigns it for a non-empty env value), so both sides resolve the
    empty allow-list through `SETTING_DEFAULTS`.
    """
    settings.CORS_ALLOWED_ORIGINS = list(getattr(settings, "CORS_ALLOWED_ORIGINS", []))
    assert _reason() is None


def test_an_in_place_list_edit_is_a_limitation(settings):
    """The baseline is a deep copy, so mutating the live list is detected."""
    origins = settings.CSRF_TRUSTED_ORIGINS
    original = list(origins)
    try:
        origins.append("https://contract-detector.example")
        reason = _reason()
        assert reason is not None
        assert "settings.CSRF_TRUSTED_ORIGINS" in reason
        assert "contract-detector.example" in reason
        assert reason.endswith(SANCTIONED)
    finally:
        # `override_settings`/`settings` shallow-copy the settings object, so an
        # in-place edit is not undone by the fixture; restore it here.
        origins[:] = original
    assert _reason() is None


@override_settings(DEBUG=False)
def test_an_unrelated_setting_is_not_a_limitation():
    assert _reason() is None
