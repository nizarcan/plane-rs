# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Synthetic URLconf for probe_http.py.

These views exist only to exercise DRF's own machinery with Plane's settings
(renderers, parsers, exception handler, metadata).  They are not Plane routes;
the route-level probes use the real ``plane.urls``.
"""

from django.http import Http404
from rest_framework import serializers
from rest_framework.authentication import BaseAuthentication
from rest_framework.exceptions import AuthenticationFailed, ParseError, PermissionDenied, ValidationError
from rest_framework.generics import GenericAPIView
from rest_framework.permissions import AllowAny, IsAuthenticated
from rest_framework.response import Response
from rest_framework.throttling import SimpleRateThrottle
from rest_framework.views import APIView

from django.urls import path

from plane.api.views.base import BaseAPIView


class PlainView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def get(self, request):
        return Response({"ok": True, "detail": "plain"})


class EchoView(APIView):
    """Reaches the parser: any POST body is parsed and echoed back."""

    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def post(self, request):
        if request.method in ("POST", "PUT", "PATCH") and not isinstance(request.data, (dict, list)):
            raise ParseError("Request body must be a JSON object or array.")
        return Response({"parsed": request.data})


class DemoSerializer(serializers.Serializer):
    name = serializers.CharField(max_length=10)
    count = serializers.IntegerField(required=False, min_value=1)
    when = serializers.DateTimeField(required=False)
    amount = serializers.DecimalField(max_digits=10, decimal_places=2, required=False)
    flag = serializers.BooleanField(required=False)
    kind = serializers.ChoiceField(choices=[("a", "A"), ("b", "B")], required=False)


class GenericView(GenericAPIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []
    serializer_class = DemoSerializer

    def get(self, request):
        return Response({"ok": True})

    def post(self, request):
        serializer = self.get_serializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        return Response(serializer.validated_data, status=201)


class PermissionView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def get(self, request):
        raise PermissionDenied()


class NotFoundView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def get(self, request):
        raise Http404


class BoomView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def get(self, request):
        raise RuntimeError("probe: unhandled exception")


class ValidationView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def post(self, request):
        serializer = DemoSerializer(data=request.data)
        serializer.is_valid(raise_exception=True)
        return Response(serializer.validated_data)


class ValidationStringView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def post(self, request):
        raise ValidationError("boom")


class ParseErrorView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = []

    def post(self, request):
        raise ParseError("explicit parse error")


class OnePerMinute(SimpleRateThrottle):
    scope = "probe"
    rate = "1/minute"

    def get_cache_key(self, request, view):
        return self.cache_format % {"scope": self.scope, "ident": self.get_ident(request)}


class ThrottledView(APIView):
    authentication_classes = []
    permission_classes = [AllowAny]
    throttle_classes = [OnePerMinute]

    def get(self, request):
        return Response({"ok": True})


class ApiKeyProbeView(BaseAPIView):
    """Plane's /api/v1/ base view, with auth/permissions relaxed so the
    ApiKeyRateThrottle response headers can be observed without a database."""

    authentication_classes = []
    permission_classes = [AllowAny]

    def get(self, request):
        return Response({"ok": True})


class NoHeaderAuthenticator(BaseAuthentication):
    """Same shape as plane.api.middleware.api_authentication.APIKeyAuthentication:
    a BaseAuthentication subclass with no authenticate_header()."""

    def authenticate(self, request):
        if request.headers.get("X-Fail") == "raise":
            raise AuthenticationFailed("Given API token is not valid")
        return None


class WithHeaderAuthenticator(NoHeaderAuthenticator):
    def authenticate_header(self, request):
        return 'Basic realm="api"'


class AuthNoneView(APIView):
    authentication_classes = [NoHeaderAuthenticator]
    permission_classes = [IsAuthenticated]
    throttle_classes = []

    def get(self, request):
        return Response({"ok": True})


class AuthHeaderView(APIView):
    authentication_classes = [WithHeaderAuthenticator]
    permission_classes = [IsAuthenticated]
    throttle_classes = []

    def get(self, request):
        return Response({"ok": True})


urlpatterns = [
    path("t/plain/", PlainView.as_view()),
    path("t/echo/", EchoView.as_view()),
    path("t/apikey/", ApiKeyProbeView.as_view()),
    path("t/auth-none/", AuthNoneView.as_view()),
    path("t/auth-header/", AuthHeaderView.as_view()),
    path("t/generic/", GenericView.as_view()),
    path("t/permission/", PermissionView.as_view()),
    path("t/notfound/", NotFoundView.as_view()),
    path("t/boom/", BoomView.as_view()),
    path("t/validation/", ValidationView.as_view()),
    path("t/validation-string/", ValidationStringView.as_view()),
    path("t/parse-error/", ParseErrorView.as_view()),
    path("t/throttled/", ThrottledView.as_view()),
]
