# Serializer inventory

Generated from DRF's own introspection by `apps/server/tools/inventory/serializers.py`. Do not edit by hand: re-run the generator. The machine-readable form is `serializers.json`; this file is the reviewable view of it.

## How this was produced

Every serializer module in `plane.app.serializers`, `plane.api.serializers`, `plane.space.serializer` and `plane.license.api.serializers` is imported, and every distinct `BaseSerializer` subclass is instantiated with no instance and no context (the way `XSerializer(qs, many=True)` does). Fields are read from `_readable_fields`, which is the order `to_representation` writes keys in, and each field's properties are recorded together with the Django model field its `source` resolves to.

`fields=` and `expand=` are recorded per serializer under `expansion`. `simple` / `dynamic` / `v1` serializers are code-generated into `crates/plane-serializers/src/generated/`; `manual` serializers have a hand-written `to_representation` (or a `get_fields`/`__init__` override, or an underivable field type) and need a hand-written Rust module, which is the next session's work. Nothing here is stubbed: a manual serializer has no generated Rust type at all.

## Counts

| Kind | Serializers |
| --- | ---: |
| simple | 146 |
| dynamic | 16 |
| v1 | 42 |
| manual | 23 |
| **total** | **227** |

2888 readable fields, 16 write-only fields recorded for Phase 5, 277 views mapped.

## Rust runtime

`crates/plane-serializers` renders a serializer object with the same shape and bytes as `JSONRenderer().render(serializer.data)`. A generated struct is plain data (`Clone, Debug`) in output order; `drf_write` checks the field mask, consults the resolved expansions and writes each field through the wrapper for its DRF class:

| DRF field class | Rust wrapper | Python |
| --- | --- | --- |
| `UUIDField`, UUID `PrimaryKeyRelatedField` | `DrfUuid` | `str(uuid)` |
| integer `PrimaryKeyRelatedField` | `i64` | JSON number |
| `PrimaryKeyRelatedField` with no resolvable model (annotation) | `DrfPk` | `str(uuid)` / number / string / `null` |
| `DateTimeField` | `DrfDateTime` in the request's active timezone | `isoformat()`, `+00:00` → `Z` |
| `DateField` / `TimeField` | `PyDate` / `PyTime` | `isoformat()` |
| `DecimalField` | `DrfDecimal` (`coerce_to_string=True`) or `EncoderDecimal::Drf` | quantised string / `float(value)` |
| `JSONField` | `PgJson` from `plane_db::JsonText` | `transcode_json` keeps Postgres key order |
| `ModelField(BinaryField)` | `PyBase64` | `b64encode(value).decode("ascii")` |
| `ListField` of UUID/str/int/float | `DrfUuidList` / `DrfStrList` / `DrfIntList` / `DrfFloatList` | a JSON array |
| `SerializerMethodField`, `ReadOnlyField` over an annotation | `DrfValue` | whatever the handler supplies |
| nested serializer | `&Nested`, `Option<&Nested>`, `&[Nested]` | an object / `null` / array |

`fields=` is `FieldEntry` (`Name` or the dict-item `Nested`; only the top-level key matters, as in `_filter_fields`). `expand=` is `Expansions`: the handler resolves every requested name to a `DrfValue` and `write_injected` appends the injectable ones after the base fields. A requested mapper name with no value is an error rather than a silently unexpanded field, matching Django's `AttributeError`.

`SkipField` is modelled as `FieldValue<T>` on annotation-backed fields with `required=False`: `Absent` omits the key, `Null` writes `null` (the attribute exists and is `None`, which DRF writes as `null` without calling the field) and `Value(v)` renders the value. A nullable model column stays `Option<T>` because its attribute always exists; the two are never mixed because `allow_null` fields are not skippable. `None` for a *value* is written as `null` by `DrfWriter::write_value`, which compensates for `JSONRenderer.render(None) == b""` only applying at the top level.

## Golden corpus

`crates/plane-serializers/tests/golden/serializers.jsonl` holds 110 renders recorded by `tools/golden/serializers_record.py` from a real Django process (`tools/golden/serializers_fixture.py` builds the rows in a `t_*` clone). `crates/plane-serializers/tests/golden.rs` builds the same values and asserts byte equality. Covered: no arguments in `UTC`, `Asia/Kolkata` and `America/New_York` (2026-11-01, the US DST fall-back), `fields=` lists and the dict form, every `expand=` shape, `fields=` and `expand=` together, `many=True`, a skippable annotation that is present and `None` (written as `null`) next to one that is missing (`SkipField`), and a probe serializer for decimal/time/binary/JSON formatting Plane's models do not exercise.

## Manual serializers

These need a hand-written Rust module (Phase 5 for most of them):

* `plane.api.serializers.base.BaseSerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.api.serializers.cycle.CycleCreateSerializer` — __init__ defined in plane.api.serializers.cycle.CycleCreateSerializer
* `plane.api.serializers.cycle.CycleUpdateSerializer` — __init__ defined in plane.api.serializers.cycle.CycleCreateSerializer
* `plane.api.serializers.issue.CycleIssueSerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.api.serializers.issue.IssueRelationResponseSerializer` — field 'blocking' has no derivable output type
* `plane.api.serializers.issue.IssueSerializer` — to_representation defined in plane.api.serializers.issue.IssueSerializer
* `plane.api.serializers.issue.ModuleIssueSerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.api.serializers.member.BaseMemberLiteAPISerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.api.serializers.module.ModuleSerializer` — to_representation defined in plane.api.serializers.module.ModuleSerializer
* `plane.app.serializers.base.BaseSerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.app.serializers.base.DynamicBaseSerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.app.serializers.draft.DraftIssueCreateSerializer` — to_representation defined in plane.app.serializers.draft.DraftIssueCreateSerializer
* `plane.app.serializers.intake.IntakeIssueDetailSerializer` — to_representation defined in plane.app.serializers.intake.IntakeIssueDetailSerializer
* `plane.app.serializers.intake.IntakeIssueSerializer` — to_representation defined in plane.app.serializers.intake.IntakeIssueSerializer
* `plane.app.serializers.issue.IssueCreateSerializer` — to_representation defined in plane.app.serializers.issue.IssueCreateSerializer
* `plane.app.serializers.issue.IssueListDetailSerializer` — `__init__` replaces `self.fields` with a list; `to_representation` is hand-written
* `plane.app.serializers.module.ModuleWriteSerializer` — to_representation defined in plane.app.serializers.module.ModuleWriteSerializer
* `plane.app.serializers.view.ViewIssueListSerializer` — to_representation defined in plane.app.serializers.view.ViewIssueListSerializer
* `plane.app.serializers.webhook.WebhookSerializer` — to_representation defined in plane.app.serializers.webhook.WebhookSerializer
* `plane.license.api.serializers.base.BaseSerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.license.api.serializers.configuration.InstanceConfigurationSerializer` — to_representation defined in plane.license.api.serializers.configuration.InstanceConfigurationSerializer
* `plane.space.serializer.base.BaseSerializer` — abstract base class: no `Meta.model`, not instantiable
* `plane.space.serializer.issue.IssueCreateSerializer` — to_representation defined in plane.space.serializer.issue.IssueCreateSerializer

## Expansion mapper

`get_expansion_mapper()` returns:

| `expand` name | Serializer | Many |
| --- | --- | --- |
| `user` | `UserLiteSerializer` | no |
| `workspace` | `WorkspaceLiteSerializer` | no |
| `project` | `ProjectLiteSerializer` | no |
| `default_assignee` | `UserLiteSerializer` | no |
| `project_lead` | `UserLiteSerializer` | no |
| `state` | `StateLiteSerializer` | no |
| `created_by` | `UserLiteSerializer` | no |
| `updated_by` | `UserLiteSerializer` | no |
| `issue` | `IssueSerializer` | no |
| `actor` | `UserLiteSerializer` | no |
| `owned_by` | `UserLiteSerializer` | no |
| `members` | `UserLiteSerializer` | yes |
| `assignees` | `UserLiteSerializer` | yes |
| `labels` | `LabelSerializer` | yes |
| `issue_cycle` | `CycleIssueSerializer` | yes |
| `parent` | `IssueLiteSerializer` | no |
| `issue_relation` | `IssueRelationSerializer` | yes |
| `issue_intake` | `IntakeIssueLiteSerializer` | yes |
| `issue_related` | `RelatedIssueSerializer` | yes |
| `issue_reactions` | `IssueReactionLiteSerializer` | yes |
| `issue_link` | `IssueLinkLiteSerializer` | yes |
| `sub_issues` | `IssueLiteSerializer` | yes |

`issue_attachments` is not in the mapper and is always appended last when requested (`plane/app/serializers/base.py:171-188`).

## Choice sets

`ChoiceField.choices` lists longer than 32 entries are stored once here and referenced from the field as `choices_ref`, instead of being repeated in every serializer that uses them. The field table still names the set and its size.

| Set | Entries |
| --- | ---: |
| `choices:adc72be4f903` | 433 |

## Serializer index

| Serializer | Model | Kind | Fields | Overrides |
| --- | --- | --- | ---: | --- |
| `AssetUpdateSerializer` (api.serializers.asset.AssetUpdateSerializer) | `` | simple | 1 |  |
| `FileAssetSerializer` (api.serializers.asset.FileAssetSerializer) | `db.FileAsset` | v1 | 25 |  |
| `GenericAssetUpdateSerializer` (api.serializers.asset.GenericAssetUpdateSerializer) | `` | simple | 1 |  |
| `GenericAssetUploadSerializer` (api.serializers.asset.GenericAssetUploadSerializer) | `` | simple | 6 |  |
| `UserAssetUploadSerializer` (api.serializers.asset.UserAssetUploadSerializer) | `` | simple | 4 |  |
| `BaseSerializer` (api.serializers.base.BaseSerializer) | `` | manual | 0 |  |
| `CycleCreateSerializer` (api.serializers.cycle.CycleCreateSerializer) | `db.Cycle` | manual | 8 | __init__@CycleCreateSerializer; validate: validate |
| `CycleIssueRequestSerializer` (api.serializers.cycle.CycleIssueRequestSerializer) | `` | simple | 1 |  |
| `CycleIssueSerializer` (api.serializers.cycle.CycleIssueSerializer) | `db.CycleIssue` | v1 | 11 |  |
| `CycleLiteSerializer` (api.serializers.cycle.CycleLiteSerializer) | `db.Cycle` | v1 | 22 |  |
| `CycleSerializer` (api.serializers.cycle.CycleSerializer) | `db.Cycle` | v1 | 31 |  |
| `CycleUpdateSerializer` (api.serializers.cycle.CycleUpdateSerializer) | `db.Cycle` | manual | 8 | __init__@CycleCreateSerializer; validate: validate |
| `TransferCycleIssueRequestSerializer` (api.serializers.cycle.TransferCycleIssueRequestSerializer) | `` | simple | 1 |  |
| `EstimatePointSerializer` (api.serializers.estimate.EstimatePointSerializer) | `db.EstimatePoint` | v1 | 12 | validate: validate |
| `EstimateSerializer` (api.serializers.estimate.EstimateSerializer) | `db.Estimate` | v1 | 12 | create@EstimateSerializer |
| `IntakeIssueCreateSerializer` (api.serializers.intake.IntakeIssueCreateSerializer) | `db.IntakeIssue` | v1 | 1 |  |
| `IntakeIssueSerializer` (api.serializers.intake.IntakeIssueSerializer) | `db.IntakeIssue` | v1 | 20 |  |
| `IntakeIssueUpdateSerializer` (api.serializers.intake.IntakeIssueUpdateSerializer) | `db.IntakeIssue` | v1 | 6 | update@IntakeIssueUpdateSerializer; validate: validate |
| `IssueDataSerializer` (api.serializers.intake.IssueDataSerializer) | `` | simple | 3 |  |
| `IssueForIntakeSerializer` (api.serializers.intake.IssueForIntakeSerializer) | `db.Issue` | v1 | 5 |  |
| `WorkspaceInviteSerializer` (api.serializers.invite.WorkspaceInviteSerializer) | `db.WorkspaceMemberInvite` | v1 | 7 | validate: validate, validate_email, validate_role |
| `CycleIssueSerializer` (api.serializers.issue.CycleIssueSerializer) | `` | manual | 0 |  |
| `IssueActivitySerializer` (api.serializers.issue.IssueActivitySerializer) | `db.IssueActivity` | v1 | 18 |  |
| `IssueAttachmentSerializer` (api.serializers.issue.IssueAttachmentSerializer) | `db.FileAsset` | v1 | 24 |  |
| `IssueAttachmentUploadSerializer` (api.serializers.issue.IssueAttachmentUploadSerializer) | `` | simple | 5 |  |
| `IssueCommentCreateSerializer` (api.serializers.issue.IssueCommentCreateSerializer) | `db.IssueComment` | v1 | 5 |  |
| `IssueCommentSerializer` (api.serializers.issue.IssueCommentSerializer) | `db.IssueComment` | v1 | 19 | validate: validate |
| `IssueExpandSerializer` (api.serializers.issue.IssueExpandSerializer) | `db.Issue` | v1 | 33 |  |
| `IssueLinkCreateSerializer` (api.serializers.issue.IssueLinkCreateSerializer) | `db.IssueLink` | v1 | 3 | create@IssueLinkCreateSerializer; validate: validate_url |
| `IssueLinkSerializer` (api.serializers.issue.IssueLinkSerializer) | `db.IssueLink` | v1 | 12 |  |
| `IssueLinkUpdateSerializer` (api.serializers.issue.IssueLinkUpdateSerializer) | `db.IssueLink` | v1 | 3 | create@IssueLinkCreateSerializer; update@IssueLinkUpdateSerializer; validate: validate_url |
| `IssueLiteSerializer` (api.serializers.issue.IssueLiteSerializer) | `db.Issue` | v1 | 3 |  |
| `IssueRelationCreateSerializer` (api.serializers.issue.IssueRelationCreateSerializer) | `` | simple | 2 | validate: validate_issues |
| `IssueRelationRefSerializer` (api.serializers.issue.IssueRelationRefSerializer) | `` | simple | 2 |  |
| `IssueRelationRemoveSerializer` (api.serializers.issue.IssueRelationRemoveSerializer) | `` | simple | 1 |  |
| `IssueRelationResponseSerializer` (api.serializers.issue.IssueRelationResponseSerializer) | `` | manual | 8 |  |
| `IssueRelationSerializer` (api.serializers.issue.IssueRelationSerializer) | `db.IssueRelation` | v1 | 11 |  |
| `IssueSearchSerializer` (api.serializers.issue.IssueSearchSerializer) | `` | simple | 6 |  |
| `IssueSerializer` (api.serializers.issue.IssueSerializer) | `db.Issue` | manual | 27 | to_representation@IssueSerializer; get_fields@IssueSerializer; create@IssueSerializer; update@IssueSerializer; validate: validate |
| `LabelCreateUpdateSerializer` (api.serializers.issue.LabelCreateUpdateSerializer) | `db.Label` | v1 | 7 |  |
| `LabelLiteSerializer` (api.serializers.issue.LabelLiteSerializer) | `db.Label` | v1 | 3 |  |
| `LabelSerializer` (api.serializers.issue.LabelSerializer) | `db.Label` | v1 | 15 |  |
| `ModuleIssueSerializer` (api.serializers.issue.ModuleIssueSerializer) | `` | manual | 0 |  |
| `RelatedIssueSerializer` (api.serializers.issue.RelatedIssueSerializer) | `db.IssueRelation` | v1 | 13 |  |
| `BaseMemberLiteAPISerializer` (api.serializers.member.BaseMemberLiteAPISerializer) | `` | manual | 0 |  |
| `ProjectMemberLiteAPISerializer` (api.serializers.member.ProjectMemberLiteAPISerializer) | `db.ProjectMember` | v1 | 10 |  |
| `ProjectMemberSerializer` (api.serializers.member.ProjectMemberSerializer) | `db.ProjectMember` | v1 | 3 | validate: validate, validate_member, validate_role |
| `WorkspaceMemberLiteAPISerializer` (api.serializers.member.WorkspaceMemberLiteAPISerializer) | `db.WorkspaceMember` | v1 | 10 |  |
| `ModuleCreateSerializer` (api.serializers.module.ModuleCreateSerializer) | `db.Module` | v1 | 8 | create@ModuleCreateSerializer; validate: validate |
| `ModuleIssueRequestSerializer` (api.serializers.module.ModuleIssueRequestSerializer) | `` | simple | 1 |  |
| `ModuleIssueSerializer` (api.serializers.module.ModuleIssueSerializer) | `db.ModuleIssue` | v1 | 11 |  |
| `ModuleLinkSerializer` (api.serializers.module.ModuleLinkSerializer) | `db.ModuleLink` | v1 | 12 | create@ModuleLinkSerializer |
| `ModuleLiteSerializer` (api.serializers.module.ModuleLiteSerializer) | `db.Module` | v1 | 23 |  |
| `ModuleSerializer` (api.serializers.module.ModuleSerializer) | `db.Module` | manual | 28 | to_representation@ModuleSerializer |
| `ModuleUpdateSerializer` (api.serializers.module.ModuleUpdateSerializer) | `db.Module` | v1 | 8 | create@ModuleCreateSerializer; update@ModuleUpdateSerializer; validate: validate |
| `ProjectCreateSerializer` (api.serializers.project.ProjectCreateSerializer) | `db.Project` | v1 | 21 | create@ProjectCreateSerializer; validate: validate |
| `ProjectLiteSerializer` (api.serializers.project.ProjectLiteSerializer) | `db.Project` | v1 | 9 |  |
| `ProjectSerializer` (api.serializers.project.ProjectSerializer) | `db.Project` | v1 | 44 | create@ProjectSerializer; validate: validate |
| `ProjectUpdateSerializer` (api.serializers.project.ProjectUpdateSerializer) | `db.Project` | v1 | 23 | create@ProjectCreateSerializer; update@ProjectUpdateSerializer; validate: validate |
| `StateLiteSerializer` (api.serializers.state.StateLiteSerializer) | `db.State` | v1 | 4 |  |
| `StateSerializer` (api.serializers.state.StateSerializer) | `db.State` | v1 | 18 | validate: validate |
| `StickySerializer` (api.serializers.sticky.StickySerializer) | `db.Sticky` | v1 | 17 | validate: validate |
| `UserLiteSerializer` (api.serializers.user.UserLiteSerializer) | `db.User` | v1 | 7 |  |
| `WorkspaceLiteSerializer` (api.serializers.workspace.WorkspaceLiteSerializer) | `db.Workspace` | v1 | 3 |  |
| `AnalyticViewSerializer` (app.serializers.analytic.AnalyticViewSerializer) | `db.AnalyticView` | simple | 11 | create@AnalyticViewSerializer; update@AnalyticViewSerializer |
| `APIActivityLogSerializer` (app.serializers.api.APIActivityLogSerializer) | `db.APIActivityLog` | simple | 16 |  |
| `APITokenReadSerializer` (app.serializers.api.APITokenReadSerializer) | `db.APIToken` | simple | 14 |  |
| `APITokenSerializer` (app.serializers.api.APITokenSerializer) | `db.APIToken` | simple | 15 |  |
| `FileAssetSerializer` (app.serializers.asset.FileAssetSerializer) | `db.FileAsset` | simple | 24 |  |
| `BaseSerializer` (app.serializers.base.BaseSerializer) | `` | manual | 0 |  |
| `DynamicBaseSerializer` (app.serializers.base.DynamicBaseSerializer) | `` | manual | 0 |  |
| `CycleIssueSerializer` (app.serializers.cycle.CycleIssueSerializer) | `db.CycleIssue` | simple | 12 |  |
| `CycleSerializer` (app.serializers.cycle.CycleSerializer) | `db.Cycle` | simple | 22 |  |
| `CycleUserPropertiesSerializer` (app.serializers.cycle.CycleUserPropertiesSerializer) | `db.CycleUserProperties` | simple | 14 |  |
| `CycleWriteSerializer` (app.serializers.cycle.CycleWriteSerializer) | `db.Cycle` | simple | 22 | validate: validate |
| `DraftIssueCreateSerializer` (app.serializers.draft.DraftIssueCreateSerializer) | `db.DraftIssue` | manual | 28 | to_representation@DraftIssueCreateSerializer; create@DraftIssueCreateSerializer; update@DraftIssueCreateSerializer; validate: validate |
| `DraftIssueDetailSerializer` (app.serializers.draft.DraftIssueDetailSerializer) | `db.DraftIssue` | simple | 21 |  |
| `DraftIssueSerializer` (app.serializers.draft.DraftIssueSerializer) | `db.DraftIssue` | simple | 21 |  |
| `EstimatePointSerializer` (app.serializers.estimate.EstimatePointSerializer) | `db.EstimatePoint` | simple | 12 | validate: validate |
| `EstimateReadSerializer` (app.serializers.estimate.EstimateReadSerializer) | `db.Estimate` | simple | 13 |  |
| `EstimateSerializer` (app.serializers.estimate.EstimateSerializer) | `db.Estimate` | simple | 12 |  |
| `WorkspaceEstimateSerializer` (app.serializers.estimate.WorkspaceEstimateSerializer) | `db.Estimate` | simple | 13 |  |
| `ExporterHistorySerializer` (app.serializers.exporter.ExporterHistorySerializer) | `db.ExporterHistory` | simple | 12 |  |
| `CycleFavoriteLiteSerializer` (app.serializers.favorite.CycleFavoriteLiteSerializer) | `db.Cycle` | simple | 4 |  |
| `ModuleFavoriteLiteSerializer` (app.serializers.favorite.ModuleFavoriteLiteSerializer) | `db.Module` | simple | 4 |  |
| `PageFavoriteLiteSerializer` (app.serializers.favorite.PageFavoriteLiteSerializer) | `db.Page` | simple | 4 |  |
| `ProjectFavoriteLiteSerializer` (app.serializers.favorite.ProjectFavoriteLiteSerializer) | `db.Project` | simple | 3 |  |
| `UserFavoriteSerializer` (app.serializers.favorite.UserFavoriteSerializer) | `db.UserFavorite` | simple | 10 |  |
| `ViewFavoriteSerializer` (app.serializers.favorite.ViewFavoriteSerializer) | `db.IssueView` | simple | 4 |  |
| `ImporterSerializer` (app.serializers.importer.ImporterSerializer) | `db.Importer` | simple | 19 |  |
| `IntakeIssueDetailSerializer` (app.serializers.intake.IntakeIssueDetailSerializer) | `db.IntakeIssue` | manual | 7 | to_representation@IntakeIssueDetailSerializer |
| `IntakeIssueLiteSerializer` (app.serializers.intake.IntakeIssueLiteSerializer) | `db.IntakeIssue` | simple | 5 |  |
| `IntakeIssueSerializer` (app.serializers.intake.IntakeIssueSerializer) | `db.IntakeIssue` | manual | 7 | to_representation@IntakeIssueSerializer; update@IntakeIssueSerializer; validate: validate |
| `IntakeSerializer` (app.serializers.intake.IntakeSerializer) | `db.Intake` | simple | 15 |  |
| `IssueStateIntakeSerializer` (app.serializers.intake.IssueStateIntakeSerializer) | `db.Issue` | simple | 36 |  |
| `CommentReactionSerializer` (app.serializers.issue.CommentReactionSerializer) | `db.CommentReaction` | simple | 12 |  |
| `CycleBaseSerializer` (app.serializers.issue.CycleBaseSerializer) | `db.Cycle` | simple | 22 |  |
| `IssueActivitySerializer` (app.serializers.issue.IssueActivitySerializer) | `db.IssueActivity` | simple | 25 |  |
| `IssueAssigneeSerializer` (app.serializers.issue.IssueAssigneeSerializer) | `db.IssueAssignee` | simple | 11 |  |
| `IssueAttachmentLiteSerializer` (app.serializers.issue.IssueAttachmentLiteSerializer) | `db.FileAsset` | dynamic | 7 |  |
| `IssueAttachmentSerializer` (app.serializers.issue.IssueAttachmentSerializer) | `db.FileAsset` | simple | 25 |  |
| `IssueCommentSerializer` (app.serializers.issue.IssueCommentSerializer) | `db.IssueComment` | simple | 26 | validate: validate |
| `IssueCreateSerializer` (app.serializers.issue.IssueCreateSerializer) | `db.Issue` | manual | 34 | to_representation@IssueCreateSerializer; get_fields@IssueCreateSerializer; create@IssueCreateSerializer; update@IssueCreateSerializer; validate: validate |
| `IssueCycleDetailSerializer` (app.serializers.issue.IssueCycleDetailSerializer) | `db.CycleIssue` | simple | 11 |  |
| `IssueDescriptionVersionDetailSerializer` (app.serializers.issue.IssueDescriptionVersionDetailSerializer) | `db.IssueDescriptionVersion` | simple | 14 |  |
| `IssueDetailSerializer` (app.serializers.issue.IssueDetailSerializer) | `db.Issue` | dynamic | 28 | validate: validate |
| `IssueFlatSerializer` (app.serializers.issue.IssueFlatSerializer) | `db.Issue` | simple | 10 |  |
| `IssueIntakeSerializer` (app.serializers.issue.IssueIntakeSerializer) | `db.Issue` | dynamic | 8 |  |
| `IssueLabelSerializer` (app.serializers.issue.IssueLabelSerializer) | `db.IssueLabel` | simple | 10 |  |
| `IssueLinkLiteSerializer` (app.serializers.issue.IssueLinkLiteSerializer) | `db.IssueLink` | simple | 7 |  |
| `IssueLinkSerializer` (app.serializers.issue.IssueLinkSerializer) | `db.IssueLink` | simple | 13 | create@IssueLinkSerializer; update@IssueLinkSerializer; validate: validate_url |
| `IssueListDetailSerializer` (app.serializers.issue.IssueListDetailSerializer) | `` | manual | 0 | to_representation@IssueListDetailSerializer; __init__@IssueListDetailSerializer |
| `IssueLiteSerializer` (app.serializers.issue.IssueLiteSerializer) | `db.Issue` | dynamic | 3 |  |
| `IssueModuleDetailSerializer` (app.serializers.issue.IssueModuleDetailSerializer) | `db.ModuleIssue` | simple | 11 |  |
| `IssueProjectLiteSerializer` (app.serializers.issue.IssueProjectLiteSerializer) | `db.Issue` | simple | 4 |  |
| `IssuePublicSerializer` (app.serializers.issue.IssuePublicSerializer) | `db.Issue` | simple | 13 |  |
| `IssueReactionLiteSerializer` (app.serializers.issue.IssueReactionLiteSerializer) | `db.IssueReaction` | dynamic | 5 |  |
| `IssueReactionSerializer` (app.serializers.issue.IssueReactionSerializer) | `db.IssueReaction` | simple | 12 |  |
| `IssueRelationSerializer` (app.serializers.issue.IssueRelationSerializer) | `db.IssueRelation` | simple | 11 |  |
| `IssueSerializer` (app.serializers.issue.IssueSerializer) | `db.Issue` | dynamic | 25 | validate: validate |
| `IssueStateFlatSerializer` (app.serializers.issue.IssueStateFlatSerializer) | `db.Issue` | simple | 5 |  |
| `IssueStateSerializer` (app.serializers.issue.IssueStateSerializer) | `db.Issue` | dynamic | 37 |  |
| `IssueSubscriberSerializer` (app.serializers.issue.IssueSubscriberSerializer) | `db.IssueSubscriber` | simple | 10 |  |
| `IssueVersionDetailSerializer` (app.serializers.issue.IssueVersionDetailSerializer) | `db.IssueVersion` | simple | 30 |  |
| `IssueVoteSerializer` (app.serializers.issue.IssueVoteSerializer) | `db.IssueVote` | simple | 6 |  |
| `LabelLiteSerializer` (app.serializers.issue.LabelLiteSerializer) | `db.Label` | simple | 3 |  |
| `LabelSerializer` (app.serializers.issue.LabelSerializer) | `db.Label` | simple | 7 | validate: validate_name |
| `ModuleBaseSerializer` (app.serializers.issue.ModuleBaseSerializer) | `db.Module` | simple | 23 |  |
| `ProjectUserPropertySerializer` (app.serializers.issue.ProjectUserPropertySerializer) | `db.ProjectUserProperty` | simple | 15 |  |
| `RelatedIssueSerializer` (app.serializers.issue.RelatedIssueSerializer) | `db.IssueRelation` | simple | 11 |  |
| `ModuleDetailSerializer` (app.serializers.module.ModuleDetailSerializer) | `db.Module` | dynamic | 35 |  |
| `ModuleFlatSerializer` (app.serializers.module.ModuleFlatSerializer) | `db.Module` | simple | 23 |  |
| `ModuleIssueSerializer` (app.serializers.module.ModuleIssueSerializer) | `db.ModuleIssue` | simple | 13 |  |
| `ModuleLinkSerializer` (app.serializers.module.ModuleLinkSerializer) | `db.ModuleLink` | simple | 12 | create@ModuleLinkSerializer; update@ModuleLinkSerializer; validate: validate_url |
| `ModuleSerializer` (app.serializers.module.ModuleSerializer) | `db.Module` | dynamic | 29 |  |
| `ModuleUserPropertiesSerializer` (app.serializers.module.ModuleUserPropertiesSerializer) | `db.ModuleUserProperties` | simple | 14 |  |
| `ModuleWriteSerializer` (app.serializers.module.ModuleWriteSerializer) | `db.Module` | manual | 24 | to_representation@ModuleWriteSerializer; create@ModuleWriteSerializer; update@ModuleWriteSerializer; validate: validate |
| `NotificationSerializer` (app.serializers.notification.NotificationSerializer) | `db.Notification` | simple | 25 |  |
| `UserNotificationPreferenceSerializer` (app.serializers.notification.UserNotificationPreferenceSerializer) | `db.UserNotificationPreference` | simple | 14 |  |
| `PageBinaryUpdateSerializer` (app.serializers.page.PageBinaryUpdateSerializer) | `` | simple | 3 | update@PageBinaryUpdateSerializer; validate: validate_description_binary, validate_description_html |
| `PageDetailSerializer` (app.serializers.page.PageDetailSerializer) | `db.Page` | simple | 19 | create@PageSerializer; update@PageSerializer |
| `PageSerializer` (app.serializers.page.PageSerializer) | `db.Page` | simple | 18 | create@PageSerializer; update@PageSerializer |
| `PageVersionDetailSerializer` (app.serializers.page.PageVersionDetailSerializer) | `db.PageVersion` | simple | 12 |  |
| `PageVersionSerializer` (app.serializers.page.PageVersionSerializer) | `db.PageVersion` | simple | 9 |  |
| `DeployBoardSerializer` (app.serializers.project.DeployBoardSerializer) | `db.DeployBoard` | simple | 20 | validate: validate_intake |
| `ProjectDetailSerializer` (app.serializers.project.ProjectDetailSerializer) | `db.Project` | simple | 40 |  |
| `ProjectIdentifierSerializer` (app.serializers.project.ProjectIdentifierSerializer) | `db.ProjectIdentifier` | simple | 9 |  |
| `ProjectListSerializer` (app.serializers.project.ProjectListSerializer) | `db.Project` | dynamic | 44 |  |
| `ProjectLiteSerializer` (app.serializers.project.ProjectLiteSerializer) | `db.Project` | simple | 7 |  |
| `ProjectMemberAdminSerializer` (app.serializers.project.ProjectMemberAdminSerializer) | `db.ProjectMember` | simple | 16 |  |
| `ProjectMemberInvitePublicSerializer` (app.serializers.project.ProjectMemberInvitePublicSerializer) | `db.ProjectMemberInvite` | simple | 7 |  |
| `ProjectMemberInviteSerializer` (app.serializers.project.ProjectMemberInviteSerializer) | `db.ProjectMemberInvite` | simple | 14 |  |
| `ProjectMemberLiteSerializer` (app.serializers.project.ProjectMemberLiteSerializer) | `db.ProjectMember` | simple | 3 |  |
| `ProjectMemberPreferenceSerializer` (app.serializers.project.ProjectMemberPreferenceSerializer) | `db.ProjectMember` | simple | 4 | validate: validate_preferences |
| `ProjectMemberRoleSerializer` (app.serializers.project.ProjectMemberRoleSerializer) | `db.ProjectMember` | dynamic | 6 |  |
| `ProjectMemberSerializer` (app.serializers.project.ProjectMemberSerializer) | `db.ProjectMember` | simple | 16 |  |
| `ProjectPublicMemberSerializer` (app.serializers.project.ProjectPublicMemberSerializer) | `db.ProjectPublicMember` | simple | 9 |  |
| `ProjectSerializer` (app.serializers.project.ProjectSerializer) | `db.Project` | simple | 38 | create@ProjectSerializer; validate: validate, validate_identifier, validate_name |
| `StateLiteSerializer` (app.serializers.state.StateLiteSerializer) | `db.State` | simple | 4 |  |
| `StateSerializer` (app.serializers.state.StateSerializer) | `db.State` | simple | 10 | validate: validate |
| `AccountSerializer` (app.serializers.user.AccountSerializer) | `db.Account` | simple | 13 |  |
| `ChangePasswordSerializer` (app.serializers.user.ChangePasswordSerializer) | `` | simple | 3 | validate: validate |
| `ProfileSerializer` (app.serializers.user.ProfileSerializer) | `db.Profile` | simple | 29 |  |
| `ResetPasswordSerializer` (app.serializers.user.ResetPasswordSerializer) | `` | simple | 1 |  |
| `UserAdminLiteSerializer` (app.serializers.user.UserAdminLiteSerializer) | `db.User` | simple | 9 |  |
| `UserLiteSerializer` (app.serializers.user.UserLiteSerializer) | `db.User` | simple | 7 |  |
| `UserMeSerializer` (app.serializers.user.UserMeSerializer) | `db.User` | simple | 18 |  |
| `UserMeSettingsSerializer` (app.serializers.user.UserMeSettingsSerializer) | `db.User` | simple | 3 |  |
| `UserSerializer` (app.serializers.user.UserSerializer) | `db.User` | simple | 39 | validate: validate_avatar_asset, validate_cover_image_asset, validate_first_name, validate_last_name |
| `IssueViewSerializer` (app.serializers.view.IssueViewSerializer) | `db.IssueView` | dynamic | 22 | create@IssueViewSerializer; update@IssueViewSerializer |
| `ViewIssueListSerializer` (app.serializers.view.ViewIssueListSerializer) | `` | manual | 0 | to_representation@ViewIssueListSerializer |
| `WebhookLogSerializer` (app.serializers.webhook.WebhookLogSerializer) | `db.WebhookLog` | dynamic | 16 |  |
| `WebhookSerializer` (app.serializers.webhook.WebhookSerializer) | `db.Webhook` | manual | 17 | to_representation@WebhookSerializer; create@WebhookSerializer; update@WebhookSerializer |
| `IssueRecentVisitSerializer` (app.serializers.workspace.IssueRecentVisitSerializer) | `db.Issue` | simple | 9 |  |
| `PageRecentVisitSerializer` (app.serializers.workspace.PageRecentVisitSerializer) | `db.Page` | simple | 6 |  |
| `ProjectRecentVisitSerializer` (app.serializers.workspace.ProjectRecentVisitSerializer) | `db.Project` | simple | 5 |  |
| `StickySerializer` (app.serializers.workspace.StickySerializer) | `db.Sticky` | simple | 17 | validate: validate |
| `WorkSpaceMemberInvitePublicSerializer` (app.serializers.workspace.WorkSpaceMemberInvitePublicSerializer) | `db.WorkspaceMemberInvite` | simple | 10 |  |
| `WorkSpaceMemberInviteSerializer` (app.serializers.workspace.WorkSpaceMemberInviteSerializer) | `db.WorkspaceMemberInvite` | simple | 14 |  |
| `WorkSpaceMemberSerializer` (app.serializers.workspace.WorkSpaceMemberSerializer) | `db.WorkspaceMember` | dynamic | 17 |  |
| `WorkSpaceSerializer` (app.serializers.workspace.WorkSpaceSerializer) | `db.Workspace` | dynamic | 17 | validate: validate_name, validate_slug |
| `WorkspaceHomePreferenceSerializer` (app.serializers.workspace.WorkspaceHomePreferenceSerializer) | `db.WorkspaceHomePreference` | simple | 3 |  |
| `WorkspaceLiteSerializer` (app.serializers.workspace.WorkspaceLiteSerializer) | `db.Workspace` | simple | 4 |  |
| `WorkspaceMemberAdminSerializer` (app.serializers.workspace.WorkspaceMemberAdminSerializer) | `db.WorkspaceMember` | dynamic | 17 |  |
| `WorkspaceMemberMeSerializer` (app.serializers.workspace.WorkspaceMemberMeSerializer) | `db.WorkspaceMember` | simple | 18 |  |
| `WorkspaceRecentVisitSerializer` (app.serializers.workspace.WorkspaceRecentVisitSerializer) | `db.UserRecentVisit` | simple | 5 |  |
| `WorkspaceThemeSerializer` (app.serializers.workspace.WorkspaceThemeSerializer) | `db.WorkspaceTheme` | simple | 10 |  |
| `WorkspaceUserLinkSerializer` (app.serializers.workspace.WorkspaceUserLinkSerializer) | `db.WorkspaceUserLink` | simple | 12 | create@WorkspaceUserLinkSerializer; update@WorkspaceUserLinkSerializer; validate: validate_url |
| `WorkspaceUserPreferenceSerializer` (app.serializers.workspace.WorkspaceUserPreferenceSerializer) | `db.WorkspaceUserPreference` | simple | 3 |  |
| `WorkspaceUserPropertiesSerializer` (app.serializers.workspace.WorkspaceUserPropertiesSerializer) | `db.WorkspaceUserProperties` | simple | 14 |  |
| `InstanceAdminMeSerializer` (license.api.serializers.admin.InstanceAdminMeSerializer) | `db.User` | simple | 15 |  |
| `InstanceAdminSerializer` (license.api.serializers.admin.InstanceAdminSerializer) | `license.InstanceAdmin` | simple | 11 |  |
| `BaseSerializer` (license.api.serializers.base.BaseSerializer) | `` | manual | 0 |  |
| `InstanceConfigurationSerializer` (license.api.serializers.configuration.InstanceConfigurationSerializer) | `license.InstanceConfiguration` | manual | 10 | to_representation@InstanceConfigurationSerializer |
| `InstanceSerializer` (license.api.serializers.instance.InstanceSerializer) | `license.Instance` | simple | 23 |  |
| `UserLiteSerializer` (license.api.serializers.user.UserLiteSerializer) | `db.User` | simple | 4 |  |
| `WorkspaceSerializer` (license.api.serializers.workspace.WorkspaceSerializer) | `db.Workspace` | simple | 17 | validate: validate_name, validate_slug |
| `BaseSerializer` (space.serializer.base.BaseSerializer) | `` | manual | 0 |  |
| `CycleBaseSerializer` (space.serializer.cycle.CycleBaseSerializer) | `db.Cycle` | simple | 22 |  |
| `IntakeIssueLiteSerializer` (space.serializer.intake.IntakeIssueLiteSerializer) | `db.IntakeIssue` | simple | 5 |  |
| `IntakeIssueSerializer` (space.serializer.intake.IntakeIssueSerializer) | `db.IntakeIssue` | simple | 20 |  |
| `IssueStateIntakeSerializer` (space.serializer.intake.IssueStateIntakeSerializer) | `db.Issue` | simple | 37 |  |
| `CommentReactionLiteSerializer` (space.serializer.issue.CommentReactionLiteSerializer) | `db.CommentReaction` | simple | 4 |  |
| `CommentReactionSerializer` (space.serializer.issue.CommentReactionSerializer) | `db.CommentReaction` | simple | 11 |  |
| `IssueAttachmentSerializer` (space.serializer.issue.IssueAttachmentSerializer) | `db.FileAsset` | simple | 24 |  |
| `IssueCommentSerializer` (space.serializer.issue.IssueCommentSerializer) | `db.IssueComment` | simple | 26 |  |
| `IssueCreateSerializer` (space.serializer.issue.IssueCreateSerializer) | `db.Issue` | manual | 32 | to_representation@IssueCreateSerializer; create@IssueCreateSerializer; update@IssueCreateSerializer; validate: validate |
| `IssueCycleDetailSerializer` (space.serializer.issue.IssueCycleDetailSerializer) | `db.CycleIssue` | simple | 11 |  |
| `IssueFlatSerializer` (space.serializer.issue.IssueFlatSerializer) | `db.Issue` | simple | 10 |  |
| `IssueLinkSerializer` (space.serializer.issue.IssueLinkSerializer) | `db.IssueLink` | simple | 13 | create@IssueLinkSerializer |
| `IssueModuleDetailSerializer` (space.serializer.issue.IssueModuleDetailSerializer) | `db.ModuleIssue` | simple | 11 |  |
| `IssueProjectLiteSerializer` (space.serializer.issue.IssueProjectLiteSerializer) | `db.Issue` | simple | 4 |  |
| `IssuePublicSerializer` (space.serializer.issue.IssuePublicSerializer) | `db.Issue` | simple | 14 |  |
| `IssueReactionSerializer` (space.serializer.issue.IssueReactionSerializer) | `db.IssueReaction` | simple | 5 |  |
| `IssueRelationSerializer` (space.serializer.issue.IssueRelationSerializer) | `db.IssueRelation` | simple | 5 |  |
| `IssueSerializer` (space.serializer.issue.IssueSerializer) | `db.Issue` | simple | 43 |  |
| `IssueStateFlatSerializer` (space.serializer.issue.IssueStateFlatSerializer) | `db.Issue` | simple | 5 |  |
| `IssueVoteSerializer` (space.serializer.issue.IssueVoteSerializer) | `db.IssueVote` | simple | 5 |  |
| `LabelLiteSerializer` (space.serializer.issue.LabelLiteSerializer) | `db.Label` | simple | 3 |  |
| `LabelSerializer` (space.serializer.issue.LabelSerializer) | `db.Label` | simple | 17 |  |
| `RelatedIssueSerializer` (space.serializer.issue.RelatedIssueSerializer) | `db.IssueRelation` | simple | 5 |  |
| `ModuleBaseSerializer` (space.serializer.module.ModuleBaseSerializer) | `db.Module` | simple | 23 |  |
| `ProjectLiteSerializer` (space.serializer.project.ProjectLiteSerializer) | `db.Project` | simple | 7 |  |
| `StateLiteSerializer` (space.serializer.state.StateLiteSerializer) | `db.State` | simple | 4 |  |
| `StateSerializer` (space.serializer.state.StateSerializer) | `db.State` | simple | 18 |  |
| `UserLiteSerializer` (space.serializer.user.UserLiteSerializer) | `db.User` | simple | 7 |  |
| `WorkspaceLiteSerializer` (space.serializer.workspace.WorkspaceLiteSerializer) | `db.Workspace` | simple | 3 |  |

## Serializers

### `plane.api.serializers.asset.AssetUpdateSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `attributes` | JSONField | `attributes` |  |  |  | json |  | jsonb/text |

### `plane.api.serializers.asset.FileAssetSerializer`

Model: `db.FileAsset` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `asset_url` | CharField | `asset_url` | yes |  |  | str | property |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `attributes` | JSONField | `attributes` |  |  |  | json | JSONField | jsonb/text |
| 7 | `asset` | FileField | `asset` |  |  |  | str | FileField |  |
| 8 | `entity_type` | CharField | `entity_type` |  |  | yes | str | CharField |  |
| 9 | `entity_identifier` | CharField | `entity_identifier` |  |  | yes | str | CharField |  |
| 10 | `is_deleted` | BooleanField | `is_deleted` | yes |  |  | bool | BooleanField |  |
| 11 | `is_archived` | BooleanField | `is_archived` |  |  |  | bool | BooleanField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 14 | `size` | FloatField | `size` |  |  |  | float | FloatField |  |
| 15 | `is_uploaded` | BooleanField | `is_uploaded` |  |  |  | bool | BooleanField |  |
| 16 | `storage_metadata` | JSONField | `storage_metadata` | yes |  | yes | json | JSONField | jsonb/text |
| 17 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 18 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `user` | PrimaryKeyRelatedField | `user` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `draft_issue` | PrimaryKeyRelatedField | `draft_issue` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `comment` | PrimaryKeyRelatedField | `comment` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 25 | `page` | PrimaryKeyRelatedField | `page` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.asset.GenericAssetUpdateSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `is_uploaded` | BooleanField | `is_uploaded` |  |  |  | bool |  |  |

### `plane.api.serializers.asset.GenericAssetUploadSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str |  |  |
| 2 | `type` | CharField | `type` |  |  |  | str |  |  |
| 3 | `size` | IntegerField | `size` |  |  |  | int |  |  |
| 4 | `project_id` | UUIDField | `project_id` |  |  |  | uuid |  |  |
| 5 | `external_id` | CharField | `external_id` |  |  |  | str |  |  |
| 6 | `external_source` | CharField | `external_source` |  |  |  | str |  |  |

### `plane.api.serializers.asset.UserAssetUploadSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str |  |  |
| 2 | `type` | ChoiceField | `type` |  |  |  | str |  | 5 choices |
| 3 | `size` | IntegerField | `size` |  |  |  | int |  |  |
| 4 | `entity_type` | ChoiceField | `entity_type` |  |  |  | str |  | 2 choices |

### `plane.api.serializers.base.BaseSerializer`

Model: `None` · base: `rest_framework.serializers.ModelSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.api.serializers.cycle.CycleCreateSerializer`

Model: `db.Cycle` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: __init__ defined in plane.api.serializers.cycle.CycleCreateSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 3 | `start_date` | DateTimeField | `start_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 4 | `end_date` | DateTimeField | `end_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 6 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 7 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 8 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |

### `plane.api.serializers.cycle.CycleIssueRequestSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issues` | ListField | `issues` |  |  |  | list<uuid> |  | child UUIDField |

### `plane.api.serializers.cycle.CycleIssueSerializer`

Model: `db.CycleIssue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 11 | `cycle` | PrimaryKeyRelatedField | `cycle` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.cycle.CycleLiteSerializer`

Model: `db.Cycle` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `start_date` | DateTimeField | `start_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `end_date` | DateTimeField | `end_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 10 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 11 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `progress_snapshot` | JSONField | `progress_snapshot` |  |  |  | json | JSONField | jsonb/text |
| 14 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 15 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 16 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 17 | `version` | IntegerField | `version` |  |  |  | int | IntegerField |  |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 22 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.cycle.CycleSerializer`

Model: `db.Cycle` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `total_issues` | IntegerField | `total_issues` | yes |  |  | int | annotation |  |
| 3 | `cancelled_issues` | IntegerField | `cancelled_issues` | yes |  |  | int | annotation |  |
| 4 | `completed_issues` | IntegerField | `completed_issues` | yes |  |  | int | annotation |  |
| 5 | `started_issues` | IntegerField | `started_issues` | yes |  |  | int | annotation |  |
| 6 | `unstarted_issues` | IntegerField | `unstarted_issues` | yes |  |  | int | annotation |  |
| 7 | `backlog_issues` | IntegerField | `backlog_issues` | yes |  |  | int | annotation |  |
| 8 | `total_estimates` | FloatField | `total_estimates` | yes |  |  | float | annotation |  |
| 9 | `completed_estimates` | FloatField | `completed_estimates` | yes |  |  | float | annotation |  |
| 10 | `started_estimates` | FloatField | `started_estimates` | yes |  |  | float | annotation |  |
| 11 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 12 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 13 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 14 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 15 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 16 | `start_date` | DateTimeField | `start_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 17 | `end_date` | DateTimeField | `end_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 18 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 19 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 20 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 21 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 22 | `progress_snapshot` | JSONField | `progress_snapshot` |  |  |  | json | JSONField | jsonb/text |
| 23 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 24 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 25 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 26 | `version` | IntegerField | `version` |  |  |  | int | IntegerField |  |
| 27 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 28 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 29 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 30 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 31 | `owned_by` | PrimaryKeyRelatedField | `owned_by` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.cycle.CycleUpdateSerializer`

Model: `db.Cycle` · base: `plane.api.serializers.cycle.CycleCreateSerializer` · codegen: **manual**

Manual because: __init__ defined in plane.api.serializers.cycle.CycleCreateSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 3 | `start_date` | DateTimeField | `start_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 4 | `end_date` | DateTimeField | `end_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 6 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 7 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 8 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |

### `plane.api.serializers.cycle.TransferCycleIssueRequestSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `new_cycle_id` | UUIDField | `new_cycle_id` |  |  |  | uuid |  |  |

### `plane.api.serializers.estimate.EstimatePointSerializer`

Model: `db.EstimatePoint` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `key` | IntegerField | `key` |  |  |  | int | IntegerField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `value` | CharField | `value` |  |  |  | str | CharField |  |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `estimate` | PrimaryKeyRelatedField | `estimate` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.estimate.EstimateSerializer`

Model: `db.Estimate` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `type` | ChoiceField | `type` |  |  |  | str | CharField | 2 choices |
| 8 | `last_used` | BooleanField | `last_used` |  |  |  | bool | BooleanField |  |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.intake.IntakeIssueCreateSerializer`

Model: `db.IntakeIssue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issue` | IssueForIntakeSerializer | `issue` |  |  |  | serializer | ForeignKey | nested IssueForIntakeSerializer |

### `plane.api.serializers.intake.IntakeIssueSerializer`

Model: `db.IntakeIssue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `issue_detail` | IssueExpandSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueExpandSerializer |
| 3 | `inbox` | UUIDField | `intake.id` | yes |  |  | uuid | UUIDField |  |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `status` | ChoiceField | `status` |  |  |  | int | IntegerField | 5 choices |
| 8 | `snoozed_till` | DateTimeField | `snoozed_till` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `source` | CharField | `source` |  |  | yes | str | CharField |  |
| 10 | `source_email` | CharField | `source_email` |  |  | yes | str | TextField |  |
| 11 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `extra` | JSONField | `extra` |  |  |  | json | JSONField | jsonb/text |
| 14 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 17 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 18 | `intake` | PrimaryKeyRelatedField | `intake` |  |  |  | uuid | ForeignKey | pk uuid |
| 19 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 20 | `duplicate_to` | PrimaryKeyRelatedField | `duplicate_to` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.intake.IntakeIssueUpdateSerializer`

Model: `db.IntakeIssue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `status` | ChoiceField | `status` |  |  |  | int | IntegerField | 5 choices |
| 2 | `snoozed_till` | DateTimeField | `snoozed_till` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 3 | `duplicate_to` | PrimaryKeyRelatedField | `duplicate_to` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `source` | CharField | `source` |  |  | yes | str | CharField |  |
| 5 | `source_email` | CharField | `source_email` |  |  | yes | str | TextField |  |
| 6 | `issue` | IssueForIntakeSerializer | `issue` |  |  |  | serializer | ForeignKey | nested IssueForIntakeSerializer |

### `plane.api.serializers.intake.IssueDataSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str |  |  |
| 2 | `description_html` | CharField | `description_html` |  |  | yes | str |  |  |
| 3 | `priority` | ChoiceField | `priority` |  |  |  | str |  | 5 choices |

### `plane.api.serializers.intake.IssueForIntakeSerializer`

Model: `db.Issue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `description` | JSONField | `description_json` |  |  | yes | json | JSONField | jsonb/text |
| 3 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 4 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 5 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |

### `plane.api.serializers.invite.WorkspaceInviteSerializer`

Model: `db.WorkspaceMemberInvite` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `email` | CharField | `email` |  |  |  | str | CharField |  |
| 3 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `responded_at` | DateTimeField | `responded_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `accepted` | BooleanField | `accepted` | yes |  |  | bool | BooleanField |  |

### `plane.api.serializers.issue.CycleIssueSerializer`

Model: `None` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.api.serializers.issue.IssueActivitySerializer`

Model: `db.IssueActivity` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `verb` | CharField | `verb` |  |  |  | str | CharField |  |
| 6 | `field` | CharField | `field` |  |  | yes | str | CharField |  |
| 7 | `old_value` | CharField | `old_value` |  |  | yes | str | TextField |  |
| 8 | `new_value` | CharField | `new_value` |  |  | yes | str | TextField |  |
| 9 | `comment` | CharField | `comment` |  |  |  | str | TextField |  |
| 10 | `attachments` | ListField | `attachments` |  |  |  | list<str> | ArrayField | child URLField |
| 11 | `old_identifier` | UUIDField | `old_identifier` |  |  | yes | uuid | UUIDField |  |
| 12 | `new_identifier` | UUIDField | `new_identifier` |  |  | yes | uuid | UUIDField |  |
| 13 | `epoch` | FloatField | `epoch` |  |  | yes | float | FloatField |  |
| 14 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 15 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 16 | `issue` | PrimaryKeyRelatedField | `issue` |  |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `issue_comment` | PrimaryKeyRelatedField | `issue_comment` |  |  | yes | uuid | ForeignKey | pk uuid |
| 18 | `actor` | PrimaryKeyRelatedField | `actor` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.issue.IssueAttachmentSerializer`

Model: `db.FileAsset` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `attributes` | JSONField | `attributes` |  |  |  | json | JSONField | jsonb/text |
| 6 | `asset` | FileField | `asset` |  |  |  | str | FileField |  |
| 7 | `entity_type` | CharField | `entity_type` |  |  | yes | str | CharField |  |
| 8 | `entity_identifier` | CharField | `entity_identifier` |  |  | yes | str | CharField |  |
| 9 | `is_deleted` | BooleanField | `is_deleted` |  |  |  | bool | BooleanField |  |
| 10 | `is_archived` | BooleanField | `is_archived` |  |  |  | bool | BooleanField |  |
| 11 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 12 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 13 | `size` | FloatField | `size` |  |  |  | float | FloatField |  |
| 14 | `is_uploaded` | BooleanField | `is_uploaded` |  |  |  | bool | BooleanField |  |
| 15 | `storage_metadata` | JSONField | `storage_metadata` |  |  | yes | json | JSONField | jsonb/text |
| 16 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 18 | `user` | PrimaryKeyRelatedField | `user` |  |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `draft_issue` | PrimaryKeyRelatedField | `draft_issue` |  |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `comment` | PrimaryKeyRelatedField | `comment` |  |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `page` | PrimaryKeyRelatedField | `page` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.issue.IssueAttachmentUploadSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str |  |  |
| 2 | `type` | CharField | `type` |  |  |  | str |  |  |
| 3 | `size` | IntegerField | `size` |  |  |  | int |  |  |
| 4 | `external_id` | CharField | `external_id` |  |  |  | str |  |  |
| 5 | `external_source` | CharField | `external_source` |  |  |  | str |  |  |

### `plane.api.serializers.issue.IssueCommentCreateSerializer`

Model: `db.IssueComment` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `comment_json` | JSONField | `comment_json` |  |  |  | json | JSONField | jsonb/text |
| 2 | `comment_html` | CharField | `comment_html` |  |  |  | str | TextField |  |
| 3 | `access` | ChoiceField | `access` |  |  |  | str | CharField | 2 choices |
| 4 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 5 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |

### `plane.api.serializers.issue.IssueCommentSerializer`

Model: `db.IssueComment` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `is_member` | BooleanField | `is_member` | yes |  |  | bool | annotation |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `comment_html` | CharField | `comment_html` |  |  |  | str | TextField |  |
| 7 | `attachments` | ListField | `attachments` |  |  |  | list<str> | ArrayField | child URLField |
| 8 | `access` | ChoiceField | `access` |  |  |  | str | CharField | 2 choices |
| 9 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 10 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 11 | `edited_at` | DateTimeField | `edited_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 12 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 15 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 16 | `description` | PrimaryKeyRelatedField | `description` |  |  | yes | uuid | OneToOneField | pk uuid |
| 17 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 18 | `actor` | PrimaryKeyRelatedField | `actor` |  |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.issue.IssueExpandSerializer`

Model: `db.Issue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `cycle` | CycleLiteSerializer | `issue_cycle.cycle` | yes |  |  | serializer | ForeignKey | nested CycleLiteSerializer |
| 3 | `module` | ModuleLiteSerializer | `issue_module.module` | yes |  |  | serializer | ForeignKey | nested ModuleLiteSerializer |
| 4 | `labels` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_labels |
| 5 | `assignees` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_assignees |
| 6 | `state` | StateLiteSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateLiteSerializer |
| 7 | `description` | JSONField | `description_json` | yes |  |  | json | JSONField | jsonb/text |
| 8 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 11 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 12 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 13 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 14 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 15 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 16 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 17 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 18 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 19 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 20 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 21 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 22 | `completed_at` | DateTimeField | `completed_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 23 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 24 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 25 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 26 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 27 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 28 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 29 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 30 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 31 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 32 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 33 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.issue.IssueLinkCreateSerializer`

Model: `db.IssueLink` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 2 | `url` | CharField | `url` |  |  |  | str | TextField |  |
| 3 | `issue_id` | ReadOnlyField | `issue_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.api.serializers.issue.IssueLinkSerializer`

Model: `db.IssueLink` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 6 | `url` | CharField | `url` |  |  |  | str | TextField |  |
| 7 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.issue.IssueLinkUpdateSerializer`

Model: `db.IssueLink` · base: `plane.api.serializers.issue.IssueLinkCreateSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 2 | `url` | CharField | `url` |  |  |  | str | TextField |  |
| 3 | `issue_id` | ReadOnlyField | `issue_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.api.serializers.issue.IssueLiteSerializer`

Model: `db.Issue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 3 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.api.serializers.issue.IssueRelationCreateSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `relation_type` | ChoiceField | `relation_type` |  |  |  | str |  | 8 choices |
| 2 | `issues` | ListField | `issues` |  |  |  | list<uuid> |  | child UUIDField |

### `plane.api.serializers.issue.IssueRelationRefSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `project_id` | UUIDField | `project_id` |  |  |  | uuid |  |  |
| 2 | `issue_id` | UUIDField | `issue_id` |  |  |  | uuid |  |  |

### `plane.api.serializers.issue.IssueRelationRemoveSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `related_issue` | UUIDField | `related_issue` |  |  |  | uuid |  |  |

### `plane.api.serializers.issue.IssueRelationResponseSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **manual**

Manual because: field 'blocking' has no derivable output type.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `blocking` | ListField | `blocking` |  |  |  |  |  | child IssueRelationRefSerializer |
| 2 | `blocked_by` | ListField | `blocked_by` |  |  |  |  |  | child IssueRelationRefSerializer |
| 3 | `duplicate` | ListField | `duplicate` |  |  |  |  |  | child IssueRelationRefSerializer |
| 4 | `relates_to` | ListField | `relates_to` |  |  |  |  |  | child IssueRelationRefSerializer |
| 5 | `start_after` | ListField | `start_after` |  |  |  |  |  | child IssueRelationRefSerializer |
| 6 | `start_before` | ListField | `start_before` |  |  |  |  |  | child IssueRelationRefSerializer |
| 7 | `finish_after` | ListField | `finish_after` |  |  |  |  |  | child IssueRelationRefSerializer |
| 8 | `finish_before` | ListField | `finish_before` |  |  |  |  |  | child IssueRelationRefSerializer |

### `plane.api.serializers.issue.IssueRelationSerializer`

Model: `db.IssueRelation` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `related_issue.id` | yes |  |  | uuid | UUIDField |  |
| 2 | `project_id` | UUIDField | `related_issue.project_id` | yes |  |  | uuid | ForeignKey |  |
| 3 | `sequence_id` | IntegerField | `related_issue.sequence_id` | yes |  |  | int | IntegerField |  |
| 4 | `relation_type` | CharField | `relation_type` | yes |  |  | str | CharField |  |
| 5 | `name` | CharField | `related_issue.name` | yes |  |  | str | CharField |  |
| 6 | `state_id` | UUIDField | `related_issue.state.id` | yes |  |  | uuid | UUIDField |  |
| 7 | `priority` | CharField | `related_issue.priority` | yes |  |  | str | CharField |  |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.issue.IssueSearchSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | CharField | `id` |  |  |  | str |  |  |
| 2 | `name` | CharField | `name` |  |  |  | str |  |  |
| 3 | `sequence_id` | CharField | `sequence_id` |  |  |  | str |  |  |
| 4 | `project__identifier` | CharField | `project__identifier` |  |  |  | str |  |  |
| 5 | `project_id` | CharField | `project_id` |  |  |  | str |  |  |
| 6 | `workspace__slug` | CharField | `workspace__slug` |  |  |  | str |  |  |

### `plane.api.serializers.issue.IssueSerializer`

Model: `db.Issue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.api.serializers.issue.IssueSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `type_id` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 7 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 8 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 9 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 10 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 11 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 12 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 13 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 14 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 15 | `completed_at` | DateTimeField | `completed_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 16 | `archived_at` | DateField | `archived_at` | yes |  | yes | date | DateField | ISO 8601 |
| 17 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 18 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 19 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 20 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 23 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 24 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 25 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 26 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 27 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `assignees` | ListField | `assignees` | PrimaryKeyRelatedField |
| `labels` | ListField | `labels` | PrimaryKeyRelatedField |

### `plane.api.serializers.issue.LabelCreateUpdateSerializer`

Model: `db.Label` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 3 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 4 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 5 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 6 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |

### `plane.api.serializers.issue.LabelLiteSerializer`

Model: `db.Label` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `color` | CharField | `color` |  |  |  | str | CharField |  |

### `plane.api.serializers.issue.LabelSerializer`

Model: `db.Label` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 8 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 9 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 10 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 11 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 14 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.issue.ModuleIssueSerializer`

Model: `None` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.api.serializers.issue.RelatedIssueSerializer`

Model: `db.IssueRelation` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `issue.id` | yes |  |  | uuid | UUIDField |  |
| 2 | `project_id` | PrimaryKeyRelatedField | `issue.project_id` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `sequence_id` | IntegerField | `issue.sequence_id` | yes |  |  | int | IntegerField |  |
| 4 | `relation_type` | CharField | `relation_type` | yes |  |  | str | CharField |  |
| 5 | `name` | CharField | `issue.name` | yes |  |  | str | CharField |  |
| 6 | `type_id` | UUIDField | `issue.type.id` | yes |  |  | uuid | UUIDField |  |
| 7 | `is_epic` | BooleanField | `issue.type.is_epic` | yes |  |  | bool | BooleanField |  |
| 8 | `state_id` | UUIDField | `issue.state.id` | yes |  |  | uuid | UUIDField |  |
| 9 | `priority` | CharField | `issue.priority` | yes |  |  | str | CharField |  |
| 10 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |

### `plane.api.serializers.member.BaseMemberLiteAPISerializer`

Model: `None` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.api.serializers.member.ProjectMemberLiteAPISerializer`

Model: `db.ProjectMember` · base: `plane.api.serializers.member.BaseMemberLiteAPISerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `member.id` | yes |  |  | uuid | UUIDField |  |
| 2 | `first_name` | CharField | `member.first_name` | yes |  |  | str | CharField |  |
| 3 | `last_name` | CharField | `member.last_name` | yes |  |  | str | CharField |  |
| 4 | `email` | EmailField | `member.email` | yes |  |  | str | CharField |  |
| 5 | `avatar` | CharField | `member.avatar` | yes |  | yes | str | TextField |  |
| 6 | `avatar_url` | CharField | `member.avatar_url` | yes |  | yes | str | property |  |
| 7 | `display_name` | CharField | `member.display_name` | yes |  |  | str | CharField |  |
| 8 | `role` | ChoiceField | `role` | yes |  |  | int | PositiveSmallIntegerField | 3 choices |
| 9 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 10 | `is_bot` | BooleanField | `member.is_bot` | yes |  |  | bool | BooleanField |  |

### `plane.api.serializers.member.ProjectMemberSerializer`

Model: `db.ProjectMember` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `member` | PrimaryKeyRelatedField | `member` |  |  |  | uuid | ForeignKey | pk uuid |
| 3 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |

### `plane.api.serializers.member.WorkspaceMemberLiteAPISerializer`

Model: `db.WorkspaceMember` · base: `plane.api.serializers.member.BaseMemberLiteAPISerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `member.id` | yes |  |  | uuid | UUIDField |  |
| 2 | `first_name` | CharField | `member.first_name` | yes |  |  | str | CharField |  |
| 3 | `last_name` | CharField | `member.last_name` | yes |  |  | str | CharField |  |
| 4 | `email` | EmailField | `member.email` | yes |  |  | str | CharField |  |
| 5 | `avatar` | CharField | `member.avatar` | yes |  | yes | str | TextField |  |
| 6 | `avatar_url` | CharField | `member.avatar_url` | yes |  | yes | str | property |  |
| 7 | `display_name` | CharField | `member.display_name` | yes |  |  | str | CharField |  |
| 8 | `role` | ChoiceField | `role` | yes |  |  | int | PositiveSmallIntegerField | 3 choices |
| 9 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 10 | `is_bot` | BooleanField | `member.is_bot` | yes |  |  | bool | BooleanField |  |

### `plane.api.serializers.module.ModuleCreateSerializer`

Model: `db.Module` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 3 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 4 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 5 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 6 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 8 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `members` | ListField | `members` | PrimaryKeyRelatedField |

### `plane.api.serializers.module.ModuleIssueRequestSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issues` | ListField | `issues` |  |  |  | list<uuid> |  | child UUIDField |

### `plane.api.serializers.module.ModuleIssueSerializer`

Model: `db.ModuleIssue` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `module` | PrimaryKeyRelatedField | `module` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.module.ModuleLinkSerializer`

Model: `db.ModuleLink` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 6 | `url` | URLField | `url` |  |  |  | str | URLField |  |
| 7 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `module` | PrimaryKeyRelatedField | `module` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.module.ModuleLiteSerializer`

Model: `db.Module` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 8 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 9 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 10 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 11 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 12 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 15 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 16 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 17 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 22 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `members` | ManyRelatedField | `members` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.api.serializers.module.ModuleSerializer`

Model: `db.Module` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.api.serializers.module.ModuleSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `total_issues` | IntegerField | `total_issues` | yes |  |  | int | annotation |  |
| 3 | `cancelled_issues` | IntegerField | `cancelled_issues` | yes |  |  | int | annotation |  |
| 4 | `completed_issues` | IntegerField | `completed_issues` | yes |  |  | int | annotation |  |
| 5 | `started_issues` | IntegerField | `started_issues` | yes |  |  | int | annotation |  |
| 6 | `unstarted_issues` | IntegerField | `unstarted_issues` | yes |  |  | int | annotation |  |
| 7 | `backlog_issues` | IntegerField | `backlog_issues` | yes |  |  | int | annotation |  |
| 8 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 11 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 12 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 13 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 14 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 15 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 16 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 17 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 18 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 19 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 20 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 21 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 22 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 23 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 24 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 25 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 26 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 27 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 28 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `members` | ListField | `members` | PrimaryKeyRelatedField |

### `plane.api.serializers.module.ModuleUpdateSerializer`

Model: `db.Module` · base: `plane.api.serializers.module.ModuleCreateSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 3 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 4 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 5 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 6 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 8 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `members` | ListField | `members` | PrimaryKeyRelatedField |

### `plane.api.serializers.project.ProjectCreateSerializer`

Model: `db.Project` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 3 | `project_lead` | PrimaryKeyRelatedField | `project_lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `default_assignee` | PrimaryKeyRelatedField | `default_assignee` |  |  | yes | uuid | ForeignKey | pk uuid |
| 5 | `identifier` | CharField | `identifier` |  |  |  | str | CharField |  |
| 6 | `icon_prop` | JSONField | `icon_prop` |  |  | yes | json | JSONField | jsonb/text |
| 7 | `emoji` | CharField | `emoji` |  |  | yes | str | CharField |  |
| 8 | `cover_image` | CharField | `cover_image` |  |  | yes | str | TextField |  |
| 9 | `module_view` | BooleanField | `module_view` |  |  |  | bool | BooleanField |  |
| 10 | `cycle_view` | BooleanField | `cycle_view` |  |  |  | bool | BooleanField |  |
| 11 | `issue_views_view` | BooleanField | `issue_views_view` |  |  |  | bool | BooleanField |  |
| 12 | `page_view` | BooleanField | `page_view` |  |  |  | bool | BooleanField |  |
| 13 | `intake_view` | BooleanField | `intake_view` |  |  |  | bool | BooleanField |  |
| 14 | `guest_view_all_features` | BooleanField | `guest_view_all_features` |  |  |  | bool | BooleanField |  |
| 15 | `archive_in` | IntegerField | `archive_in` |  |  |  | int | IntegerField |  |
| 16 | `close_in` | IntegerField | `close_in` |  |  |  | int | IntegerField |  |
| 17 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 18 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 19 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 20 | `is_issue_type_enabled` | BooleanField | `is_issue_type_enabled` |  |  |  | bool | BooleanField |  |
| 21 | `is_time_tracking_enabled` | BooleanField | `is_time_tracking_enabled` |  |  |  | bool | BooleanField |  |

### `plane.api.serializers.project.ProjectLiteSerializer`

Model: `db.Project` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `identifier` | CharField | `identifier` | yes |  |  | str | CharField |  |
| 3 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 4 | `cover_image` | CharField | `cover_image` | yes |  | yes | str | TextField |  |
| 5 | `icon_prop` | JSONField | `icon_prop` | yes |  | yes | json | JSONField | jsonb/text |
| 6 | `emoji` | CharField | `emoji` | yes |  | yes | str | CharField |  |
| 7 | `description` | CharField | `description` | yes |  |  | str | TextField |  |
| 8 | `cover_image_url` | CharField | `cover_image_url` | yes |  |  | str | property |  |
| 9 | `archived_at` | DateTimeField | `archived_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |

### `plane.api.serializers.project.ProjectSerializer`

Model: `db.Project` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `total_members` | IntegerField | `total_members` | yes |  |  | int | annotation |  |
| 3 | `total_cycles` | IntegerField | `total_cycles` | yes |  |  | int | annotation |  |
| 4 | `total_modules` | IntegerField | `total_modules` | yes |  |  | int | annotation |  |
| 5 | `is_member` | BooleanField | `is_member` | yes |  |  | bool | annotation |  |
| 6 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | annotation |  |
| 7 | `member_role` | IntegerField | `member_role` | yes |  |  | int | annotation |  |
| 8 | `is_deployed` | BooleanField | `is_deployed` | yes |  |  | bool | annotation |  |
| 9 | `cover_image_url` | CharField | `cover_image_url` | yes |  |  | str | property |  |
| 10 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 12 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 13 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 14 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 15 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 16 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 17 | `network` | ChoiceField | `network` |  |  |  | int | PositiveSmallIntegerField | 2 choices |
| 18 | `identifier` | CharField | `identifier` |  |  |  | str | CharField |  |
| 19 | `emoji` | CharField | `emoji` | yes |  | yes | str | CharField |  |
| 20 | `icon_prop` | JSONField | `icon_prop` |  |  | yes | json | JSONField | jsonb/text |
| 21 | `module_view` | BooleanField | `module_view` |  |  |  | bool | BooleanField |  |
| 22 | `cycle_view` | BooleanField | `cycle_view` |  |  |  | bool | BooleanField |  |
| 23 | `issue_views_view` | BooleanField | `issue_views_view` |  |  |  | bool | BooleanField |  |
| 24 | `page_view` | BooleanField | `page_view` |  |  |  | bool | BooleanField |  |
| 25 | `intake_view` | BooleanField | `intake_view` |  |  |  | bool | BooleanField |  |
| 26 | `is_time_tracking_enabled` | BooleanField | `is_time_tracking_enabled` |  |  |  | bool | BooleanField |  |
| 27 | `is_issue_type_enabled` | BooleanField | `is_issue_type_enabled` |  |  |  | bool | BooleanField |  |
| 28 | `guest_view_all_features` | BooleanField | `guest_view_all_features` |  |  |  | bool | BooleanField |  |
| 29 | `cover_image` | CharField | `cover_image` |  |  | yes | str | TextField |  |
| 30 | `archive_in` | IntegerField | `archive_in` |  |  |  | int | IntegerField |  |
| 31 | `close_in` | IntegerField | `close_in` |  |  |  | int | IntegerField |  |
| 32 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 33 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 34 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 35 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 36 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 37 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 38 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 39 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 40 | `default_assignee` | PrimaryKeyRelatedField | `default_assignee` |  |  | yes | uuid | ForeignKey | pk uuid |
| 41 | `project_lead` | PrimaryKeyRelatedField | `project_lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 42 | `cover_image_asset` | PrimaryKeyRelatedField | `cover_image_asset` |  |  | yes | uuid | ForeignKey | pk uuid |
| 43 | `estimate` | PrimaryKeyRelatedField | `estimate` |  |  | yes | uuid | ForeignKey | pk uuid |
| 44 | `default_state` | PrimaryKeyRelatedField | `default_state` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.project.ProjectUpdateSerializer`

Model: `db.Project` · base: `plane.api.serializers.project.ProjectCreateSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 2 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 3 | `project_lead` | PrimaryKeyRelatedField | `project_lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `default_assignee` | PrimaryKeyRelatedField | `default_assignee` |  |  | yes | uuid | ForeignKey | pk uuid |
| 5 | `identifier` | CharField | `identifier` |  |  |  | str | CharField |  |
| 6 | `icon_prop` | JSONField | `icon_prop` |  |  | yes | json | JSONField | jsonb/text |
| 7 | `emoji` | CharField | `emoji` |  |  | yes | str | CharField |  |
| 8 | `cover_image` | CharField | `cover_image` |  |  | yes | str | TextField |  |
| 9 | `module_view` | BooleanField | `module_view` |  |  |  | bool | BooleanField |  |
| 10 | `cycle_view` | BooleanField | `cycle_view` |  |  |  | bool | BooleanField |  |
| 11 | `issue_views_view` | BooleanField | `issue_views_view` |  |  |  | bool | BooleanField |  |
| 12 | `page_view` | BooleanField | `page_view` |  |  |  | bool | BooleanField |  |
| 13 | `intake_view` | BooleanField | `intake_view` |  |  |  | bool | BooleanField |  |
| 14 | `guest_view_all_features` | BooleanField | `guest_view_all_features` |  |  |  | bool | BooleanField |  |
| 15 | `archive_in` | IntegerField | `archive_in` |  |  |  | int | IntegerField |  |
| 16 | `close_in` | IntegerField | `close_in` |  |  |  | int | IntegerField |  |
| 17 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 18 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 19 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 20 | `is_issue_type_enabled` | BooleanField | `is_issue_type_enabled` |  |  |  | bool | BooleanField |  |
| 21 | `is_time_tracking_enabled` | BooleanField | `is_time_tracking_enabled` |  |  |  | bool | BooleanField |  |
| 22 | `default_state` | PrimaryKeyRelatedField | `default_state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `estimate` | PrimaryKeyRelatedField | `estimate` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.state.StateLiteSerializer`

Model: `db.State` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `color` | CharField | `color` | yes |  |  | str | CharField |  |
| 4 | `group` | ChoiceField | `group` | yes |  |  | str | CharField | 6 choices |

### `plane.api.serializers.state.StateSerializer`

Model: `db.State` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 8 | `slug` | SlugField | `slug` | yes |  |  | str | SlugField |  |
| 9 | `sequence` | FloatField | `sequence` |  |  |  | float | FloatField |  |
| 10 | `group` | ChoiceField | `group` |  |  |  | str | CharField | 6 choices |
| 11 | `is_triage` | BooleanField | `is_triage` |  |  |  | bool | BooleanField |  |
| 12 | `default` | BooleanField | `default` |  |  |  | bool | BooleanField |  |
| 13 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 14 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 18 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.sticky.StickySerializer`

Model: `db.Sticky` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  | yes | str | TextField |  |
| 6 | `description` | JSONField | `description` |  |  |  | json | JSONField | jsonb/text |
| 7 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 8 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 9 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 10 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `color` | CharField | `color` |  |  | yes | str | CharField |  |
| 12 | `background_color` | CharField | `background_color` |  |  | yes | str | CharField |  |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 17 | `owner` | PrimaryKeyRelatedField | `owner` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.api.serializers.user.UserLiteSerializer`

Model: `db.User` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `first_name` | CharField | `first_name` | yes |  |  | str | CharField |  |
| 3 | `last_name` | CharField | `last_name` | yes |  |  | str | CharField |  |
| 4 | `email` | CharField | `email` | yes |  | yes | str | CharField |  |
| 5 | `avatar` | CharField | `avatar` | yes |  |  | str | TextField |  |
| 6 | `avatar_url` | CharField | `avatar_url` | yes |  |  | str | property |  |
| 7 | `display_name` | CharField | `display_name` | yes |  |  | str | CharField |  |

### `plane.api.serializers.workspace.WorkspaceLiteSerializer`

Model: `db.Workspace` · base: `plane.api.serializers.base.BaseSerializer` · codegen: **v1**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 2 | `slug` | SlugField | `slug` | yes |  |  | str | SlugField |  |
| 3 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |

### `plane.app.serializers.analytic.AnalyticViewSerializer`

Model: `db.AnalyticView` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `query` | JSONField | `query` | yes |  |  | json | JSONField | jsonb/text |
| 8 | `query_dict` | JSONField | `query_dict` |  |  |  | json | JSONField | jsonb/text |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.api.APIActivityLogSerializer`

Model: `db.APIActivityLog` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `token_identifier` | CharField | `token_identifier` |  |  |  | str | CharField |  |
| 6 | `path` | CharField | `path` |  |  |  | str | CharField |  |
| 7 | `method` | CharField | `method` |  |  |  | str | CharField |  |
| 8 | `query_params` | CharField | `query_params` |  |  | yes | str | TextField |  |
| 9 | `headers` | CharField | `headers` |  |  | yes | str | TextField |  |
| 10 | `body` | CharField | `body` |  |  | yes | str | TextField |  |
| 11 | `response_code` | IntegerField | `response_code` |  |  |  | int | PositiveIntegerField |  |
| 12 | `response_body` | CharField | `response_body` |  |  | yes | str | TextField |  |
| 13 | `ip_address` | IPAddressField | `ip_address` |  |  | yes | str | GenericIPAddressField |  |
| 14 | `user_agent` | CharField | `user_agent` |  |  | yes | str | CharField |  |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.api.APITokenReadSerializer`

Model: `db.APIToken` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `is_active` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_is_active |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `label` | CharField | `label` |  |  |  | str | CharField |  |
| 7 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 8 | `last_used` | DateTimeField | `last_used` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `user_type` | ChoiceField | `user_type` |  |  |  | int | PositiveSmallIntegerField | 2 choices |
| 10 | `expired_at` | DateTimeField | `expired_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 11 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `user` | PrimaryKeyRelatedField | `user` |  |  |  | uuid | ForeignKey | pk uuid |
| 14 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.api.APITokenSerializer`

Model: `db.APIToken` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `label` | CharField | `label` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 8 | `last_used` | DateTimeField | `last_used` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `token` | CharField | `token` | yes |  |  | str | CharField |  |
| 10 | `user_type` | ChoiceField | `user_type` | yes |  |  | int | PositiveSmallIntegerField | 2 choices |
| 11 | `expired_at` | DateTimeField | `expired_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 12 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | ForeignKey | pk uuid |
| 15 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.asset.FileAssetSerializer`

Model: `db.FileAsset` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `attributes` | JSONField | `attributes` |  |  |  | json | JSONField | jsonb/text |
| 6 | `asset` | FileField | `asset` |  |  |  | str | FileField |  |
| 7 | `entity_type` | CharField | `entity_type` | yes |  | yes | str | CharField |  |
| 8 | `entity_identifier` | CharField | `entity_identifier` | yes |  | yes | str | CharField |  |
| 9 | `is_deleted` | BooleanField | `is_deleted` | yes |  |  | bool | BooleanField |  |
| 10 | `is_archived` | BooleanField | `is_archived` | yes |  |  | bool | BooleanField |  |
| 11 | `external_id` | CharField | `external_id` | yes |  | yes | str | CharField |  |
| 12 | `external_source` | CharField | `external_source` | yes |  | yes | str | CharField |  |
| 13 | `size` | FloatField | `size` | yes |  |  | float | FloatField |  |
| 14 | `is_uploaded` | BooleanField | `is_uploaded` | yes |  |  | bool | BooleanField |  |
| 15 | `storage_metadata` | JSONField | `storage_metadata` | yes |  | yes | json | JSONField | jsonb/text |
| 16 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 18 | `user` | PrimaryKeyRelatedField | `user` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `draft_issue` | PrimaryKeyRelatedField | `draft_issue` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `comment` | PrimaryKeyRelatedField | `comment` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `page` | PrimaryKeyRelatedField | `page` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.base.BaseSerializer`

Model: `None` · base: `rest_framework.serializers.ModelSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.app.serializers.base.DynamicBaseSerializer`

Model: `None` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.app.serializers.cycle.CycleIssueSerializer`

Model: `db.CycleIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `issue_detail` | IssueStateSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueStateSerializer |
| 3 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 12 | `cycle` | PrimaryKeyRelatedField | `cycle` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.cycle.CycleSerializer`

Model: `db.Cycle` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace_id` | ReadOnlyField | `workspace_id` | yes |  |  | uuid | ForeignKey |  |
| 3 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 5 | `description` | CharField | `description` | yes |  |  | str | TextField |  |
| 6 | `start_date` | DateTimeField | `start_date` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `end_date` | DateTimeField | `end_date` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `owned_by_id` | ReadOnlyField | `owned_by_id` | yes |  |  | uuid | ForeignKey |  |
| 9 | `view_props` | JSONField | `view_props` | yes |  |  | json | JSONField | jsonb/text |
| 10 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | FloatField |  |
| 11 | `external_source` | CharField | `external_source` | yes |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` | yes |  | yes | str | CharField |  |
| 13 | `progress_snapshot` | JSONField | `progress_snapshot` | yes |  |  | json | JSONField | jsonb/text |
| 14 | `logo_props` | JSONField | `logo_props` | yes |  |  | json | JSONField | jsonb/text |
| 15 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 16 | `total_issues` | IntegerField | `total_issues` | yes |  |  | int | annotation |  |
| 17 | `cancelled_issues` | IntegerField | `cancelled_issues` | yes |  |  | int | annotation |  |
| 18 | `completed_issues` | IntegerField | `completed_issues` | yes |  |  | int | annotation |  |
| 19 | `started_issues` | IntegerField | `started_issues` | yes |  |  | int | annotation |  |
| 20 | `unstarted_issues` | IntegerField | `unstarted_issues` | yes |  |  | int | annotation |  |
| 21 | `backlog_issues` | IntegerField | `backlog_issues` | yes |  |  | int | annotation |  |
| 22 | `status` | CharField | `status` | yes |  |  | str | annotation |  |

### `plane.app.serializers.cycle.CycleUserPropertiesSerializer`

Model: `db.CycleUserProperties` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `filters` | JSONField | `filters` |  |  |  | json | JSONField | jsonb/text |
| 6 | `display_filters` | JSONField | `display_filters` |  |  |  | json | JSONField | jsonb/text |
| 7 | `display_properties` | JSONField | `display_properties` |  |  |  | json | JSONField | jsonb/text |
| 8 | `rich_filters` | JSONField | `rich_filters` |  |  |  | json | JSONField | jsonb/text |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 13 | `cycle` | PrimaryKeyRelatedField | `cycle` | yes |  |  | uuid | ForeignKey | pk uuid |
| 14 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.cycle.CycleWriteSerializer`

Model: `db.Cycle` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `start_date` | DateTimeField | `start_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `end_date` | DateTimeField | `end_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 10 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 11 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `progress_snapshot` | JSONField | `progress_snapshot` |  |  |  | json | JSONField | jsonb/text |
| 14 | `archived_at` | DateTimeField | `archived_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 15 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 16 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 17 | `version` | IntegerField | `version` |  |  |  | int | IntegerField |  |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `owned_by` | PrimaryKeyRelatedField | `owned_by` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.draft.DraftIssueCreateSerializer`

Model: `db.DraftIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.app.serializers.draft.DraftIssueCreateSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `state_id` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 3 | `parent_id` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `name` | CharField | `name` |  |  | yes | str | CharField |  |
| 8 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 9 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 10 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 11 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 12 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 13 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 14 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 15 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 16 | `completed_at` | DateTimeField | `completed_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 17 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 18 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 19 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 25 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 26 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 27 | `assignees` | ManyRelatedField | `assignees` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |
| 28 | `labels` | ManyRelatedField | `labels` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `label_ids` | ListField | `label_ids` | PrimaryKeyRelatedField |
| `assignee_ids` | ListField | `assignee_ids` | PrimaryKeyRelatedField |

### `plane.app.serializers.draft.DraftIssueDetailSerializer`

Model: `db.DraftIssue` · base: `plane.app.serializers.draft.DraftIssueSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  | yes | str | CharField |  |
| 3 | `state_id` | ReadOnlyField | `state_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | FloatField |  |
| 5 | `completed_at` | DateTimeField | `completed_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `priority` | ChoiceField | `priority` | yes |  |  | str | CharField | 5 choices |
| 8 | `start_date` | DateField | `start_date` | yes |  | yes | date | DateField | ISO 8601 |
| 9 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 10 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 11 | `parent_id` | ReadOnlyField | `parent_id` | yes |  |  | uuid | ForeignKey |  |
| 12 | `cycle_id` | PrimaryKeyRelatedField | `cycle_id` | yes |  |  | pk | annotation | pk None |
| 13 | `module_ids` | ListField | `module_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 14 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 15 | `assignee_ids` | ListField | `assignee_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 16 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 17 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `type_id` | ReadOnlyField | `type_id` | yes |  |  | uuid | ForeignKey |  |
| 21 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |

### `plane.app.serializers.draft.DraftIssueSerializer`

Model: `db.DraftIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  | yes | str | CharField |  |
| 3 | `state_id` | ReadOnlyField | `state_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | FloatField |  |
| 5 | `completed_at` | DateTimeField | `completed_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `priority` | ChoiceField | `priority` | yes |  |  | str | CharField | 5 choices |
| 8 | `start_date` | DateField | `start_date` | yes |  | yes | date | DateField | ISO 8601 |
| 9 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 10 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 11 | `parent_id` | ReadOnlyField | `parent_id` | yes |  |  | uuid | ForeignKey |  |
| 12 | `cycle_id` | PrimaryKeyRelatedField | `cycle_id` | yes |  |  | pk | annotation | pk None |
| 13 | `module_ids` | ListField | `module_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 14 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 15 | `assignee_ids` | ListField | `assignee_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 16 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 17 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `type_id` | ReadOnlyField | `type_id` | yes |  |  | uuid | ForeignKey |  |
| 21 | `description_html` | CharField | `description_html` | yes |  |  | str | TextField |  |

### `plane.app.serializers.estimate.EstimatePointSerializer`

Model: `db.EstimatePoint` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `key` | IntegerField | `key` |  |  |  | int | IntegerField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `value` | CharField | `value` |  |  |  | str | CharField |  |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `estimate` | PrimaryKeyRelatedField | `estimate` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.estimate.EstimateReadSerializer`

Model: `db.Estimate` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `points` | ListSerializer | `points` | yes |  |  | list<serializer> | ManyToOneRel | many nested EstimatePointSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 7 | `description` | CharField | `description` | yes |  |  | str | TextField |  |
| 8 | `type` | ChoiceField | `type` |  |  |  | str | CharField | 2 choices |
| 9 | `last_used` | BooleanField | `last_used` |  |  |  | bool | BooleanField |  |
| 10 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 13 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.estimate.EstimateSerializer`

Model: `db.Estimate` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `type` | ChoiceField | `type` |  |  |  | str | CharField | 2 choices |
| 8 | `last_used` | BooleanField | `last_used` |  |  |  | bool | BooleanField |  |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.estimate.WorkspaceEstimateSerializer`

Model: `db.Estimate` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `points` | ListSerializer | `points` | yes |  |  | list<serializer> | ManyToOneRel | many nested EstimatePointSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 7 | `description` | CharField | `description` | yes |  |  | str | TextField |  |
| 8 | `type` | ChoiceField | `type` |  |  |  | str | CharField | 2 choices |
| 9 | `last_used` | BooleanField | `last_used` |  |  |  | bool | BooleanField |  |
| 10 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 13 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.exporter.ExporterHistorySerializer`

Model: `db.ExporterHistory` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `project` | ListField | `project` | yes |  | yes | list<uuid> | ArrayField | child UUIDField |
| 5 | `provider` | ChoiceField | `provider` | yes |  |  | str | CharField | 3 choices |
| 6 | `status` | ChoiceField | `status` | yes |  |  | str | CharField | 4 choices |
| 7 | `url` | URLField | `url` | yes |  | yes | str | URLField |  |
| 8 | `initiated_by` | PrimaryKeyRelatedField | `initiated_by` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `initiated_by_detail` | UserLiteSerializer | `initiated_by` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 10 | `token` | CharField | `token` | yes |  |  | str | CharField |  |
| 11 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.favorite.CycleFavoriteLiteSerializer`

Model: `db.Cycle` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 4 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.app.serializers.favorite.ModuleFavoriteLiteSerializer`

Model: `db.Module` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 4 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.app.serializers.favorite.PageFavoriteLiteSerializer`

Model: `db.Page` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | TextField |  |
| 3 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 4 | `project_id` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_project_id |

### `plane.app.serializers.favorite.ProjectFavoriteLiteSerializer`

Model: `db.Project` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |

### `plane.app.serializers.favorite.UserFavoriteSerializer`

Model: `db.UserFavorite` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `entity_type` | CharField | `entity_type` |  |  |  | str | CharField |  |
| 3 | `entity_identifier` | UUIDField | `entity_identifier` |  |  | yes | uuid | UUIDField |  |
| 4 | `entity_data` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_entity_data |
| 5 | `name` | CharField | `name` |  |  | yes | str | CharField |  |
| 6 | `is_folder` | BooleanField | `is_folder` |  |  |  | bool | BooleanField |  |
| 7 | `sequence` | FloatField | `sequence` |  |  |  | float | FloatField |  |
| 8 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `workspace_id` | ReadOnlyField | `workspace_id` | yes |  |  | uuid | ForeignKey |  |
| 10 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.app.serializers.favorite.ViewFavoriteSerializer`

Model: `db.IssueView` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 4 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.app.serializers.importer.ImporterSerializer`

Model: `db.Importer` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `initiated_by_detail` | UserLiteSerializer | `initiated_by` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 5 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `service` | ChoiceField | `service` |  |  |  | str | CharField | 2 choices |
| 9 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 4 choices |
| 10 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 11 | `config` | JSONField | `config` |  |  |  | json | JSONField | jsonb/text |
| 12 | `data` | JSONField | `data` |  |  |  | json | JSONField | jsonb/text |
| 13 | `imported_data` | JSONField | `imported_data` |  |  | yes | json | JSONField | jsonb/text |
| 14 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 17 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 18 | `initiated_by` | PrimaryKeyRelatedField | `initiated_by` |  |  |  | uuid | ForeignKey | pk uuid |
| 19 | `token` | PrimaryKeyRelatedField | `token` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.intake.IntakeIssueDetailSerializer`

Model: `db.IntakeIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.app.serializers.intake.IntakeIssueDetailSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `status` | ChoiceField | `status` |  |  |  | int | IntegerField | 5 choices |
| 3 | `duplicate_to` | PrimaryKeyRelatedField | `duplicate_to` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `snoozed_till` | DateTimeField | `snoozed_till` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `duplicate_issue_detail` | IssueIntakeSerializer | `duplicate_to` | yes |  |  | serializer | ForeignKey | nested IssueIntakeSerializer |
| 6 | `source` | CharField | `source` |  |  | yes | str | CharField |  |
| 7 | `issue` | IssueDetailSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueDetailSerializer |

### `plane.app.serializers.intake.IntakeIssueLiteSerializer`

Model: `db.IntakeIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `status` | ChoiceField | `status` | yes |  |  | int | IntegerField | 5 choices |
| 3 | `duplicate_to` | PrimaryKeyRelatedField | `duplicate_to` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `snoozed_till` | DateTimeField | `snoozed_till` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `source` | CharField | `source` | yes |  | yes | str | CharField |  |

### `plane.app.serializers.intake.IntakeIssueSerializer`

Model: `db.IntakeIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.app.serializers.intake.IntakeIssueSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `status` | ChoiceField | `status` |  |  |  | int | IntegerField | 5 choices |
| 3 | `duplicate_to` | PrimaryKeyRelatedField | `duplicate_to` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `snoozed_till` | DateTimeField | `snoozed_till` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `source` | CharField | `source` |  |  | yes | str | CharField |  |
| 6 | `issue` | IssueIntakeSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueIntakeSerializer |
| 7 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.intake.IntakeSerializer`

Model: `db.Intake` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 3 | `pending_issue_count` | IntegerField | `pending_issue_count` | yes |  |  | int | annotation |  |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 8 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 9 | `is_default` | BooleanField | `is_default` |  |  |  | bool | BooleanField |  |
| 10 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 12 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 15 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.intake.IssueStateIntakeSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `state_detail` | StateLiteSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateLiteSerializer |
| 3 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `label_details` | ListSerializer | `labels` | yes |  |  | list<serializer> | ManyToManyField | many nested LabelLiteSerializer |
| 5 | `assignee_details` | ListSerializer | `assignees` | yes |  |  | list<serializer> | ManyToManyField | many nested UserLiteSerializer |
| 6 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 7 | `issue_intake` | ListSerializer | `issue_intake` | yes |  |  | list<serializer> | ManyToOneRel | many nested IntakeIssueLiteSerializer |
| 8 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 11 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 12 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 13 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 14 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 15 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 16 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 17 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 18 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 19 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 20 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 21 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 22 | `completed_at` | DateTimeField | `completed_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 23 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 24 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 25 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 26 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 27 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 28 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 29 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 30 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 31 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 32 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 33 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 34 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 35 | `assignees` | ManyRelatedField | `assignees` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |
| 36 | `labels` | ManyRelatedField | `labels` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.app.serializers.issue.CommentReactionSerializer`

Model: `db.CommentReaction` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `actor` | PrimaryKeyRelatedField | `actor` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `comment` | PrimaryKeyRelatedField | `comment` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `reaction` | CharField | `reaction` |  |  |  | str | TextField |  |
| 5 | `display_name` | CharField | `actor.display_name` | yes |  |  | str | CharField |  |
| 6 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.CycleBaseSerializer`

Model: `db.Cycle` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `start_date` | DateTimeField | `start_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `end_date` | DateTimeField | `end_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 10 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 11 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `progress_snapshot` | JSONField | `progress_snapshot` |  |  |  | json | JSONField | jsonb/text |
| 14 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 15 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 16 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 17 | `version` | IntegerField | `version` |  |  |  | int | IntegerField |  |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueActivitySerializer`

Model: `db.IssueActivity` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `actor_detail` | UserLiteSerializer | `actor` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `issue_detail` | IssueFlatSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueFlatSerializer |
| 4 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 5 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 6 | `source_data` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_source_data |
| 7 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 8 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 10 | `verb` | CharField | `verb` |  |  |  | str | CharField |  |
| 11 | `field` | CharField | `field` |  |  | yes | str | CharField |  |
| 12 | `old_value` | CharField | `old_value` |  |  | yes | str | TextField |  |
| 13 | `new_value` | CharField | `new_value` |  |  | yes | str | TextField |  |
| 14 | `comment` | CharField | `comment` |  |  |  | str | TextField |  |
| 15 | `attachments` | ListField | `attachments` |  |  |  | list<str> | ArrayField | child URLField |
| 16 | `old_identifier` | UUIDField | `old_identifier` |  |  | yes | uuid | UUIDField |  |
| 17 | `new_identifier` | UUIDField | `new_identifier` |  |  | yes | uuid | UUIDField |  |
| 18 | `epoch` | FloatField | `epoch` |  |  | yes | float | FloatField |  |
| 19 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 22 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 23 | `issue` | PrimaryKeyRelatedField | `issue` |  |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `issue_comment` | PrimaryKeyRelatedField | `issue_comment` |  |  | yes | uuid | ForeignKey | pk uuid |
| 25 | `actor` | PrimaryKeyRelatedField | `actor` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueAssigneeSerializer`

Model: `db.IssueAssignee` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `assignee_details` | UserLiteSerializer | `assignee` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 10 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 11 | `assignee` | PrimaryKeyRelatedField | `assignee` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueAttachmentLiteSerializer`

Model: `db.FileAsset` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `asset` | FileField | `asset` | yes |  |  | str | FileField |  |
| 3 | `attributes` | JSONField | `attributes` | yes |  |  | json | JSONField | jsonb/text |
| 4 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `asset_url` | ReadOnlyField | `asset_url` | yes |  |  | str | property |  |

### `plane.app.serializers.issue.IssueAttachmentSerializer`

Model: `db.FileAsset` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `asset_url` | CharField | `asset_url` | yes |  |  | str | property |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `attributes` | JSONField | `attributes` |  |  |  | json | JSONField | jsonb/text |
| 7 | `asset` | FileField | `asset` |  |  |  | str | FileField |  |
| 8 | `entity_type` | CharField | `entity_type` |  |  | yes | str | CharField |  |
| 9 | `entity_identifier` | CharField | `entity_identifier` |  |  | yes | str | CharField |  |
| 10 | `is_deleted` | BooleanField | `is_deleted` |  |  |  | bool | BooleanField |  |
| 11 | `is_archived` | BooleanField | `is_archived` |  |  |  | bool | BooleanField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 14 | `size` | FloatField | `size` |  |  |  | float | FloatField |  |
| 15 | `is_uploaded` | BooleanField | `is_uploaded` |  |  |  | bool | BooleanField |  |
| 16 | `storage_metadata` | JSONField | `storage_metadata` |  |  | yes | json | JSONField | jsonb/text |
| 17 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 18 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `user` | PrimaryKeyRelatedField | `user` |  |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `draft_issue` | PrimaryKeyRelatedField | `draft_issue` |  |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `comment` | PrimaryKeyRelatedField | `comment` |  |  | yes | uuid | ForeignKey | pk uuid |
| 25 | `page` | PrimaryKeyRelatedField | `page` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueCommentSerializer`

Model: `db.IssueComment` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `actor_detail` | UserLiteSerializer | `actor` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `issue_detail` | IssueFlatSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueFlatSerializer |
| 4 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 5 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 6 | `comment_reactions` | ListSerializer | `comment_reactions` | yes |  |  | list<serializer> | ManyToOneRel | many nested CommentReactionSerializer |
| 7 | `is_member` | BooleanField | `is_member` | yes |  |  | bool | annotation |  |
| 8 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 11 | `comment_stripped` | CharField | `comment_stripped` |  |  |  | str | TextField |  |
| 12 | `comment_json` | JSONField | `comment_json` |  |  |  | json | JSONField | jsonb/text |
| 13 | `comment_html` | CharField | `comment_html` |  |  |  | str | TextField |  |
| 14 | `attachments` | ListField | `attachments` |  |  |  | list<str> | ArrayField | child URLField |
| 15 | `access` | ChoiceField | `access` |  |  |  | str | CharField | 2 choices |
| 16 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 17 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 18 | `edited_at` | DateTimeField | `edited_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 19 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 23 | `description` | PrimaryKeyRelatedField | `description` |  |  | yes | uuid | OneToOneField | pk uuid |
| 24 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 25 | `actor` | PrimaryKeyRelatedField | `actor` |  |  | yes | uuid | ForeignKey | pk uuid |
| 26 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueCreateSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.app.serializers.issue.IssueCreateSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `state_id` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 3 | `parent_id` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `project_id` | UUIDField | `project.id` | yes |  |  | uuid | UUIDField |  |
| 5 | `workspace_id` | UUIDField | `workspace.id` | yes |  |  | uuid | UUIDField |  |
| 6 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 8 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 10 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 11 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 12 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 13 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 14 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 15 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 16 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 17 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 18 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 19 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 20 | `completed_at` | DateTimeField | `completed_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 21 | `archived_at` | DateField | `archived_at` | yes |  | yes | date | DateField | ISO 8601 |
| 22 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 23 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 24 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 25 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 26 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 27 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 28 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 29 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 30 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 31 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 32 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 33 | `assignees` | ManyRelatedField | `assignees` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |
| 34 | `labels` | ManyRelatedField | `labels` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `label_ids` | ListField | `label_ids` | PrimaryKeyRelatedField |
| `assignee_ids` | ListField | `assignee_ids` | PrimaryKeyRelatedField |

### `plane.app.serializers.issue.IssueCycleDetailSerializer`

Model: `db.CycleIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `cycle_detail` | CycleBaseSerializer | `cycle` | yes |  |  | serializer | ForeignKey | nested CycleBaseSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 11 | `cycle` | PrimaryKeyRelatedField | `cycle` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueDescriptionVersionDetailSerializer`

Model: `db.IssueDescriptionVersion` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 5 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 6 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 7 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 8 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 9 | `last_saved_at` | DateTimeField | `last_saved_at` |  |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |
| 11 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 12 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 13 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueDetailSerializer`

Model: `db.Issue` · base: `plane.app.serializers.issue.IssueSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `state_id` | ReadOnlyField | `state_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | FloatField |  |
| 5 | `completed_at` | DateTimeField | `completed_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `priority` | ChoiceField | `priority` | yes |  |  | str | CharField | 5 choices |
| 8 | `start_date` | DateField | `start_date` | yes |  | yes | date | DateField | ISO 8601 |
| 9 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 10 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 11 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 12 | `parent_id` | ReadOnlyField | `parent_id` | yes |  |  | uuid | ForeignKey |  |
| 13 | `cycle_id` | PrimaryKeyRelatedField | `cycle_id` | yes |  |  | pk | annotation | pk None |
| 14 | `module_ids` | ListField | `module_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 15 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 16 | `assignee_ids` | ListField | `assignee_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 17 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 18 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 19 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 20 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `attachment_count` | IntegerField | `attachment_count` | yes |  |  | int | annotation |  |
| 23 | `link_count` | IntegerField | `link_count` | yes |  |  | int | annotation |  |
| 24 | `is_draft` | BooleanField | `is_draft` | yes |  |  | bool | BooleanField |  |
| 25 | `archived_at` | DateField | `archived_at` | yes |  | yes | date | DateField | ISO 8601 |
| 26 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 27 | `is_subscribed` | BooleanField | `is_subscribed` | yes |  |  | bool | annotation |  |
| 28 | `is_intake` | BooleanField | `is_intake` | yes |  |  | bool | annotation |  |

### `plane.app.serializers.issue.IssueFlatSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 4 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 5 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 6 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 7 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 8 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 9 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 10 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |

### `plane.app.serializers.issue.IssueIntakeSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `priority` | ChoiceField | `priority` | yes |  |  | str | CharField | 5 choices |
| 4 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 5 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 6 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueLabelSerializer`

Model: `db.IssueLabel` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 6 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 8 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 10 | `label` | PrimaryKeyRelatedField | `label` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueLinkLiteSerializer`

Model: `db.IssueLink` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `issue_id` | ReadOnlyField | `issue_id` | yes |  |  | uuid | ForeignKey |  |
| 3 | `title` | CharField | `title` | yes |  | yes | str | CharField |  |
| 4 | `url` | CharField | `url` | yes |  |  | str | TextField |  |
| 5 | `metadata` | JSONField | `metadata` | yes |  |  | json | JSONField | jsonb/text |
| 6 | `created_by_id` | ReadOnlyField | `created_by_id` | yes |  |  | uuid | ForeignKey |  |
| 7 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |

### `plane.app.serializers.issue.IssueLinkSerializer`

Model: `db.IssueLink` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_by_detail` | UserLiteSerializer | `created_by` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 7 | `url` | CharField | `url` |  |  |  | str | TextField |  |
| 8 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 13 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueListDetailSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **manual**

Manual because: `__init__` replaces `self.fields` with a list; `to_representation` is hand-written.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.app.serializers.issue.IssueLiteSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 3 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.app.serializers.issue.IssueModuleDetailSerializer`

Model: `db.ModuleIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `module_detail` | ModuleBaseSerializer | `module` | yes |  |  | serializer | ForeignKey | nested ModuleBaseSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `module` | PrimaryKeyRelatedField | `module` |  |  |  | uuid | ForeignKey | pk uuid |
| 11 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueProjectLiteSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 3 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 4 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |

### `plane.app.serializers.issue.IssuePublicSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `description_html` | CharField | `description_html` | yes |  |  | str | TextField |  |
| 4 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 5 | `state` | PrimaryKeyRelatedField | `state` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 6 | `state_detail` | StateLiteSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateLiteSerializer |
| 7 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 8 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `priority` | ChoiceField | `priority` | yes |  |  | str | CharField | 5 choices |
| 11 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 12 | `reactions` | ListSerializer | `issue_reactions` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueReactionSerializer |
| 13 | `votes` | ListSerializer | `votes` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueVoteSerializer |

### `plane.app.serializers.issue.IssueReactionLiteSerializer`

Model: `db.IssueReaction` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `actor` | PrimaryKeyRelatedField | `actor` |  |  |  | uuid | ForeignKey | pk uuid |
| 3 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 4 | `reaction` | CharField | `reaction` |  |  |  | str | TextField |  |
| 5 | `display_name` | CharField | `actor.display_name` | yes |  |  | str | CharField |  |

### `plane.app.serializers.issue.IssueReactionSerializer`

Model: `db.IssueReaction` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `actor_detail` | UserLiteSerializer | `actor` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `reaction` | CharField | `reaction` |  |  |  | str | TextField |  |
| 7 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `actor` | PrimaryKeyRelatedField | `actor` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueRelationSerializer`

Model: `db.IssueRelation` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `related_issue.id` | yes |  |  | uuid | UUIDField |  |
| 2 | `project_id` | PrimaryKeyRelatedField | `related_issue.project_id` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `sequence_id` | IntegerField | `related_issue.sequence_id` | yes |  |  | int | IntegerField |  |
| 4 | `relation_type` | CharField | `relation_type` | yes |  |  | str | CharField |  |
| 5 | `name` | CharField | `related_issue.name` | yes |  |  | str | CharField |  |
| 6 | `state_id` | UUIDField | `related_issue.state.id` | yes |  |  | uuid | UUIDField |  |
| 7 | `priority` | CharField | `related_issue.priority` | yes |  |  | str | CharField |  |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `assignee_ids` | ListField | `assignee_ids` | PrimaryKeyRelatedField |

### `plane.app.serializers.issue.IssueSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `state_id` | ReadOnlyField | `state_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | FloatField |  |
| 5 | `completed_at` | DateTimeField | `completed_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `priority` | ChoiceField | `priority` | yes |  |  | str | CharField | 5 choices |
| 8 | `start_date` | DateField | `start_date` | yes |  | yes | date | DateField | ISO 8601 |
| 9 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 10 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 11 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 12 | `parent_id` | ReadOnlyField | `parent_id` | yes |  |  | uuid | ForeignKey |  |
| 13 | `cycle_id` | PrimaryKeyRelatedField | `cycle_id` | yes |  |  | pk | annotation | pk None |
| 14 | `module_ids` | ListField | `module_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 15 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 16 | `assignee_ids` | ListField | `assignee_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 17 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 18 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 19 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 20 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `attachment_count` | IntegerField | `attachment_count` | yes |  |  | int | annotation |  |
| 23 | `link_count` | IntegerField | `link_count` | yes |  |  | int | annotation |  |
| 24 | `is_draft` | BooleanField | `is_draft` | yes |  |  | bool | BooleanField |  |
| 25 | `archived_at` | DateField | `archived_at` | yes |  | yes | date | DateField | ISO 8601 |

### `plane.app.serializers.issue.IssueStateFlatSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 3 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 4 | `state_detail` | StateLiteSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateLiteSerializer |
| 5 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |

### `plane.app.serializers.issue.IssueStateSerializer`

Model: `db.Issue` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `label_details` | ListSerializer | `labels` | yes |  |  | list<serializer> | ManyToManyField | many nested LabelLiteSerializer |
| 3 | `state_detail` | StateLiteSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateLiteSerializer |
| 4 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 5 | `assignee_details` | ListSerializer | `assignees` | yes |  |  | list<serializer> | ManyToManyField | many nested UserLiteSerializer |
| 6 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 7 | `attachment_count` | IntegerField | `attachment_count` | yes |  |  | int | annotation |  |
| 8 | `link_count` | IntegerField | `link_count` | yes |  |  | int | annotation |  |
| 9 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 12 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 13 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 14 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 15 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 16 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 17 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 18 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 19 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 20 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 21 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 22 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 23 | `completed_at` | DateTimeField | `completed_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 24 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 25 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 26 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 27 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 28 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 29 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 30 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 31 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 32 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 33 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 34 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 35 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 36 | `assignees` | ManyRelatedField | `assignees` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |
| 37 | `labels` | ManyRelatedField | `labels` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.app.serializers.issue.IssueSubscriberSerializer`

Model: `db.IssueSubscriber` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 6 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 8 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `subscriber` | PrimaryKeyRelatedField | `subscriber` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueVersionDetailSerializer`

Model: `db.IssueVersion` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 5 | `parent` | UUIDField | `parent` |  |  | yes | uuid | UUIDField |  |
| 6 | `state` | UUIDField | `state` |  |  | yes | uuid | UUIDField |  |
| 7 | `estimate_point` | UUIDField | `estimate_point` |  |  | yes | uuid | UUIDField |  |
| 8 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 9 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 10 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 11 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 12 | `assignees` | ListField | `assignees` |  |  |  | list<uuid> | ArrayField | child UUIDField |
| 13 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 14 | `labels` | ListField | `labels` |  |  |  | list<uuid> | ArrayField | child UUIDField |
| 15 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 16 | `completed_at` | DateTimeField | `completed_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 17 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 18 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 19 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 20 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 21 | `type` | UUIDField | `type` |  |  | yes | uuid | UUIDField |  |
| 22 | `cycle` | UUIDField | `cycle` |  |  | yes | uuid | UUIDField |  |
| 23 | `modules` | ListField | `modules` |  |  |  | list<uuid> | ArrayField | child UUIDField |
| 24 | `meta` | JSONField | `meta` |  |  |  | json | JSONField | jsonb/text |
| 25 | `last_saved_at` | DateTimeField | `last_saved_at` |  |  |  | datetime | DateTimeField | ISO 8601 |
| 26 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |
| 27 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 28 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 29 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 30 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.IssueVoteSerializer`

Model: `db.IssueVote` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 2 | `vote` | ChoiceField | `vote` | yes |  |  | int | IntegerField | 2 choices |
| 3 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 5 | `actor` | PrimaryKeyRelatedField | `actor` | yes |  |  | uuid | ForeignKey | pk uuid |
| 6 | `actor_detail` | UserLiteSerializer | `actor` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |

### `plane.app.serializers.issue.LabelLiteSerializer`

Model: `db.Label` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `color` | CharField | `color` |  |  |  | str | CharField |  |

### `plane.app.serializers.issue.LabelSerializer`

Model: `db.Label` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 4 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 5 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 6 | `workspace_id` | ReadOnlyField | `workspace_id` | yes |  |  | uuid | ForeignKey |  |
| 7 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |

### `plane.app.serializers.issue.ModuleBaseSerializer`

Model: `db.Module` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 8 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 9 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 10 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 11 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 12 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 15 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 16 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 17 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `members` | ManyRelatedField | `members` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.app.serializers.issue.ProjectUserPropertySerializer`

Model: `db.ProjectUserProperty` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `filters` | JSONField | `filters` |  |  |  | json | JSONField | jsonb/text |
| 6 | `display_filters` | JSONField | `display_filters` |  |  |  | json | JSONField | jsonb/text |
| 7 | `display_properties` | JSONField | `display_properties` |  |  |  | json | JSONField | jsonb/text |
| 8 | `rich_filters` | JSONField | `rich_filters` |  |  |  | json | JSONField | jsonb/text |
| 9 | `preferences` | JSONField | `preferences` |  |  |  | json | JSONField | jsonb/text |
| 10 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 11 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 14 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 15 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.issue.RelatedIssueSerializer`

Model: `db.IssueRelation` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `issue.id` | yes |  |  | uuid | UUIDField |  |
| 2 | `project_id` | PrimaryKeyRelatedField | `issue.project_id` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `sequence_id` | IntegerField | `issue.sequence_id` | yes |  |  | int | IntegerField |  |
| 4 | `relation_type` | CharField | `relation_type` | yes |  |  | str | CharField |  |
| 5 | `name` | CharField | `issue.name` | yes |  |  | str | CharField |  |
| 6 | `state_id` | UUIDField | `issue.state.id` | yes |  |  | uuid | UUIDField |  |
| 7 | `priority` | CharField | `issue.priority` | yes |  |  | str | CharField |  |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `assignee_ids` | ListField | `assignee_ids` | PrimaryKeyRelatedField |

### `plane.app.serializers.module.ModuleDetailSerializer`

Model: `db.Module` · base: `plane.app.serializers.module.ModuleSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace_id` | ReadOnlyField | `workspace_id` | yes |  |  | uuid | ForeignKey |  |
| 3 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 5 | `description` | CharField | `description` | yes |  |  | str | TextField |  |
| 6 | `description_text` | JSONField | `description_text` | yes |  | yes | json | JSONField | jsonb/text |
| 7 | `description_html` | JSONField | `description_html` | yes |  | yes | json | JSONField | jsonb/text |
| 8 | `start_date` | DateField | `start_date` | yes |  | yes | date | DateField | ISO 8601 |
| 9 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 10 | `status` | ChoiceField | `status` | yes |  |  | str | CharField | 6 choices |
| 11 | `lead_id` | ReadOnlyField | `lead_id` | yes |  |  | uuid | ForeignKey |  |
| 12 | `member_ids` | ListField | `member_ids` |  |  | yes | list<uuid> | annotation | child UUIDField |
| 13 | `view_props` | JSONField | `view_props` | yes |  |  | json | JSONField | jsonb/text |
| 14 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | FloatField |  |
| 15 | `external_source` | CharField | `external_source` | yes |  | yes | str | CharField |  |
| 16 | `external_id` | CharField | `external_id` | yes |  | yes | str | CharField |  |
| 17 | `logo_props` | JSONField | `logo_props` | yes |  |  | json | JSONField | jsonb/text |
| 18 | `total_estimate_points` | FloatField | `total_estimate_points` | yes |  |  | float | annotation |  |
| 19 | `completed_estimate_points` | FloatField | `completed_estimate_points` | yes |  |  | float | annotation |  |
| 20 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 21 | `total_issues` | IntegerField | `total_issues` | yes |  |  | int | annotation |  |
| 22 | `cancelled_issues` | IntegerField | `cancelled_issues` | yes |  |  | int | annotation |  |
| 23 | `completed_issues` | IntegerField | `completed_issues` | yes |  |  | int | annotation |  |
| 24 | `started_issues` | IntegerField | `started_issues` | yes |  |  | int | annotation |  |
| 25 | `unstarted_issues` | IntegerField | `unstarted_issues` | yes |  |  | int | annotation |  |
| 26 | `backlog_issues` | IntegerField | `backlog_issues` | yes |  |  | int | annotation |  |
| 27 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 28 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 29 | `archived_at` | DateTimeField | `archived_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 30 | `link_module` | ListSerializer | `link_module` | yes |  |  | list<serializer> | ManyToOneRel | many nested ModuleLinkSerializer |
| 31 | `sub_issues` | IntegerField | `sub_issues` | yes |  |  | int | annotation |  |
| 32 | `backlog_estimate_points` | FloatField | `backlog_estimate_points` | yes |  |  | float | annotation |  |
| 33 | `unstarted_estimate_points` | FloatField | `unstarted_estimate_points` | yes |  |  | float | annotation |  |
| 34 | `started_estimate_points` | FloatField | `started_estimate_points` | yes |  |  | float | annotation |  |
| 35 | `cancelled_estimate_points` | FloatField | `cancelled_estimate_points` | yes |  |  | float | annotation |  |

### `plane.app.serializers.module.ModuleFlatSerializer`

Model: `db.Module` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 8 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 9 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 10 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 11 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 12 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 15 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 16 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 17 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `members` | ManyRelatedField | `members` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.app.serializers.module.ModuleIssueSerializer`

Model: `db.ModuleIssue` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `module_detail` | ModuleFlatSerializer | `module` | yes |  |  | serializer | ForeignKey | nested ModuleFlatSerializer |
| 3 | `issue_detail` | ProjectLiteSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 5 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `module` | PrimaryKeyRelatedField | `module` | yes |  |  | uuid | ForeignKey | pk uuid |
| 13 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.module.ModuleLinkSerializer`

Model: `db.ModuleLink` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 6 | `url` | URLField | `url` |  |  |  | str | URLField |  |
| 7 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `module` | PrimaryKeyRelatedField | `module` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.module.ModuleSerializer`

Model: `db.Module` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace_id` | ReadOnlyField | `workspace_id` | yes |  |  | uuid | ForeignKey |  |
| 3 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 5 | `description` | CharField | `description` | yes |  |  | str | TextField |  |
| 6 | `description_text` | JSONField | `description_text` | yes |  | yes | json | JSONField | jsonb/text |
| 7 | `description_html` | JSONField | `description_html` | yes |  | yes | json | JSONField | jsonb/text |
| 8 | `start_date` | DateField | `start_date` | yes |  | yes | date | DateField | ISO 8601 |
| 9 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 10 | `status` | ChoiceField | `status` | yes |  |  | str | CharField | 6 choices |
| 11 | `lead_id` | ReadOnlyField | `lead_id` | yes |  |  | uuid | ForeignKey |  |
| 12 | `member_ids` | ListField | `member_ids` |  |  | yes | list<uuid> | annotation | child UUIDField |
| 13 | `view_props` | JSONField | `view_props` | yes |  |  | json | JSONField | jsonb/text |
| 14 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | FloatField |  |
| 15 | `external_source` | CharField | `external_source` | yes |  | yes | str | CharField |  |
| 16 | `external_id` | CharField | `external_id` | yes |  | yes | str | CharField |  |
| 17 | `logo_props` | JSONField | `logo_props` | yes |  |  | json | JSONField | jsonb/text |
| 18 | `total_estimate_points` | FloatField | `total_estimate_points` | yes |  |  | float | annotation |  |
| 19 | `completed_estimate_points` | FloatField | `completed_estimate_points` | yes |  |  | float | annotation |  |
| 20 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 21 | `total_issues` | IntegerField | `total_issues` | yes |  |  | int | annotation |  |
| 22 | `cancelled_issues` | IntegerField | `cancelled_issues` | yes |  |  | int | annotation |  |
| 23 | `completed_issues` | IntegerField | `completed_issues` | yes |  |  | int | annotation |  |
| 24 | `started_issues` | IntegerField | `started_issues` | yes |  |  | int | annotation |  |
| 25 | `unstarted_issues` | IntegerField | `unstarted_issues` | yes |  |  | int | annotation |  |
| 26 | `backlog_issues` | IntegerField | `backlog_issues` | yes |  |  | int | annotation |  |
| 27 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 28 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 29 | `archived_at` | DateTimeField | `archived_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |

### `plane.app.serializers.module.ModuleUserPropertiesSerializer`

Model: `db.ModuleUserProperties` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `filters` | JSONField | `filters` |  |  |  | json | JSONField | jsonb/text |
| 6 | `display_filters` | JSONField | `display_filters` |  |  |  | json | JSONField | jsonb/text |
| 7 | `display_properties` | JSONField | `display_properties` |  |  |  | json | JSONField | jsonb/text |
| 8 | `rich_filters` | JSONField | `rich_filters` |  |  |  | json | JSONField | jsonb/text |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 13 | `module` | PrimaryKeyRelatedField | `module` | yes |  |  | uuid | ForeignKey | pk uuid |
| 14 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.module.ModuleWriteSerializer`

Model: `db.Module` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.app.serializers.module.ModuleWriteSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `lead_id` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 7 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 8 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 9 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 10 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 11 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 12 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 13 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 14 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 15 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 16 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 17 | `archived_at` | DateTimeField | `archived_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 18 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 19 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 23 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `members` | ManyRelatedField | `members` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `member_ids` | ListField | `member_ids` | PrimaryKeyRelatedField |

### `plane.app.serializers.notification.NotificationSerializer`

Model: `db.Notification` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `triggered_by_details` | UserLiteSerializer | `triggered_by` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `is_inbox_issue` | BooleanField | `is_inbox_issue` | yes |  |  | bool | annotation |  |
| 4 | `is_intake_issue` | BooleanField | `is_intake_issue` | yes |  |  | bool | annotation |  |
| 5 | `is_mentioned_notification` | BooleanField | `is_mentioned_notification` | yes |  |  | bool | annotation |  |
| 6 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 8 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `data` | JSONField | `data` |  |  | yes | json | JSONField | jsonb/text |
| 10 | `entity_identifier` | UUIDField | `entity_identifier` |  |  | yes | uuid | UUIDField |  |
| 11 | `entity_name` | CharField | `entity_name` |  |  |  | str | CharField |  |
| 12 | `title` | CharField | `title` |  |  |  | str | TextField |  |
| 13 | `message` | JSONField | `message` |  |  | yes | json | JSONField | jsonb/text |
| 14 | `message_html` | CharField | `message_html` |  |  |  | str | TextField |  |
| 15 | `message_stripped` | CharField | `message_stripped` |  |  | yes | str | TextField |  |
| 16 | `sender` | CharField | `sender` |  |  |  | str | CharField |  |
| 17 | `read_at` | DateTimeField | `read_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 18 | `snoozed_till` | DateTimeField | `snoozed_till` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 19 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 20 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 23 | `project` | PrimaryKeyRelatedField | `project` |  |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `triggered_by` | PrimaryKeyRelatedField | `triggered_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 25 | `receiver` | PrimaryKeyRelatedField | `receiver` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.notification.UserNotificationPreferenceSerializer`

Model: `db.UserNotificationPreference` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `property_change` | BooleanField | `property_change` |  |  |  | bool | BooleanField |  |
| 6 | `state_change` | BooleanField | `state_change` |  |  |  | bool | BooleanField |  |
| 7 | `comment` | BooleanField | `comment` |  |  |  | bool | BooleanField |  |
| 8 | `mention` | BooleanField | `mention` |  |  |  | bool | BooleanField |  |
| 9 | `issue_completed` | BooleanField | `issue_completed` |  |  |  | bool | BooleanField |  |
| 10 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | ForeignKey | pk uuid |
| 13 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.page.PageBinaryUpdateSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `description_binary` | CharField | `description_binary` |  |  |  | str |  |  |
| 2 | `description_html` | CharField | `description_html` |  |  |  | str |  |  |
| 3 | `description_json` | JSONField | `description_json` |  |  | yes | json |  | jsonb/text |

### `plane.app.serializers.page.PageDetailSerializer`

Model: `db.Page` · base: `plane.app.serializers.page.PageSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` |  |  |  | str | TextField |  |
| 3 | `owned_by` | PrimaryKeyRelatedField | `owned_by` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `access` | ChoiceField | `access` |  |  |  | int | PositiveSmallIntegerField | 2 choices |
| 5 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 6 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 8 | `is_locked` | BooleanField | `is_locked` |  |  |  | bool | BooleanField |  |
| 9 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 10 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 12 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 13 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 16 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 17 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 18 | `project_ids` | ListField | `project_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 19 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `labels` | ListField | `labels` | PrimaryKeyRelatedField |

### `plane.app.serializers.page.PageSerializer`

Model: `db.Page` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` |  |  |  | str | TextField |  |
| 3 | `owned_by` | PrimaryKeyRelatedField | `owned_by` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `access` | ChoiceField | `access` |  |  |  | int | PositiveSmallIntegerField | 2 choices |
| 5 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 6 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 8 | `is_locked` | BooleanField | `is_locked` |  |  |  | bool | BooleanField |  |
| 9 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 10 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 12 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 13 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 16 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 17 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 18 | `project_ids` | ListField | `project_ids` |  |  |  | list<uuid> | annotation | child UUIDField |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `labels` | ListField | `labels` | PrimaryKeyRelatedField |

### `plane.app.serializers.page.PageVersionDetailSerializer`

Model: `db.PageVersion` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `page` | PrimaryKeyRelatedField | `page` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `last_saved_at` | DateTimeField | `last_saved_at` |  |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 6 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 7 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 8 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |
| 9 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.page.PageVersionSerializer`

Model: `db.PageVersion` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 3 | `page` | PrimaryKeyRelatedField | `page` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `last_saved_at` | DateTimeField | `last_saved_at` |  |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |
| 6 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.DeployBoardSerializer`

Model: `db.DeployBoard` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project_details` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 3 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `entity_identifier` | UUIDField | `entity_identifier` | yes |  | yes | uuid | UUIDField |  |
| 8 | `entity_name` | CharField | `entity_name` | yes |  | yes | str | CharField |  |
| 9 | `anchor` | CharField | `anchor` | yes |  |  | str | CharField |  |
| 10 | `is_comments_enabled` | BooleanField | `is_comments_enabled` |  |  |  | bool | BooleanField |  |
| 11 | `is_reactions_enabled` | BooleanField | `is_reactions_enabled` |  |  |  | bool | BooleanField |  |
| 12 | `is_votes_enabled` | BooleanField | `is_votes_enabled` |  |  |  | bool | BooleanField |  |
| 13 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 14 | `is_activity_enabled` | BooleanField | `is_activity_enabled` |  |  |  | bool | BooleanField |  |
| 15 | `is_disabled` | BooleanField | `is_disabled` |  |  |  | bool | BooleanField |  |
| 16 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 18 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 19 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `intake` | PrimaryKeyRelatedField | `intake` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.ProjectDetailSerializer`

Model: `db.Project` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `default_assignee` | UserLiteSerializer | `default_assignee` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `project_lead` | UserLiteSerializer | `project_lead` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 4 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 5 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | annotation |  |
| 6 | `member_role` | IntegerField | `member_role` | yes |  |  | int | annotation |  |
| 7 | `anchor` | CharField | `anchor` | yes |  |  | str | annotation |  |
| 8 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 11 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 12 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 13 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 14 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 15 | `network` | ChoiceField | `network` |  |  |  | int | PositiveSmallIntegerField | 2 choices |
| 16 | `identifier` | CharField | `identifier` |  |  |  | str | CharField |  |
| 17 | `emoji` | CharField | `emoji` |  |  | yes | str | CharField |  |
| 18 | `icon_prop` | JSONField | `icon_prop` |  |  | yes | json | JSONField | jsonb/text |
| 19 | `module_view` | BooleanField | `module_view` |  |  |  | bool | BooleanField |  |
| 20 | `cycle_view` | BooleanField | `cycle_view` |  |  |  | bool | BooleanField |  |
| 21 | `issue_views_view` | BooleanField | `issue_views_view` |  |  |  | bool | BooleanField |  |
| 22 | `page_view` | BooleanField | `page_view` |  |  |  | bool | BooleanField |  |
| 23 | `intake_view` | BooleanField | `intake_view` |  |  |  | bool | BooleanField |  |
| 24 | `is_time_tracking_enabled` | BooleanField | `is_time_tracking_enabled` |  |  |  | bool | BooleanField |  |
| 25 | `is_issue_type_enabled` | BooleanField | `is_issue_type_enabled` |  |  |  | bool | BooleanField |  |
| 26 | `guest_view_all_features` | BooleanField | `guest_view_all_features` |  |  |  | bool | BooleanField |  |
| 27 | `cover_image` | CharField | `cover_image` |  |  | yes | str | TextField |  |
| 28 | `archive_in` | IntegerField | `archive_in` |  |  |  | int | IntegerField |  |
| 29 | `close_in` | IntegerField | `close_in` |  |  |  | int | IntegerField |  |
| 30 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 31 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 32 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 33 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 34 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 35 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 36 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 37 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 38 | `cover_image_asset` | PrimaryKeyRelatedField | `cover_image_asset` |  |  | yes | uuid | ForeignKey | pk uuid |
| 39 | `estimate` | PrimaryKeyRelatedField | `estimate` |  |  | yes | uuid | ForeignKey | pk uuid |
| 40 | `default_state` | PrimaryKeyRelatedField | `default_state` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.ProjectIdentifierSerializer`

Model: `db.ProjectIdentifier` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | int | BigAutoField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | OneToOneField | pk uuid |

### `plane.app.serializers.project.ProjectListSerializer`

Model: `db.Project` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 3 | `sort_order` | FloatField | `sort_order` | yes |  |  | float | annotation |  |
| 4 | `member_role` | IntegerField | `member_role` | yes |  |  | int | annotation |  |
| 5 | `anchor` | CharField | `anchor` | yes |  |  | str | annotation |  |
| 6 | `members` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_members |
| 7 | `cover_image_url` | CharField | `cover_image_url` | yes |  |  | str | property |  |
| 8 | `inbox_view` | BooleanField | `intake_view` | yes |  |  | bool | BooleanField |  |
| 9 | `next_work_item_sequence` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_next_work_item_sequence |
| 10 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 12 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 13 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 14 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 15 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 16 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 17 | `network` | ChoiceField | `network` |  |  |  | int | PositiveSmallIntegerField | 2 choices |
| 18 | `identifier` | CharField | `identifier` |  |  |  | str | CharField |  |
| 19 | `emoji` | CharField | `emoji` |  |  | yes | str | CharField |  |
| 20 | `icon_prop` | JSONField | `icon_prop` |  |  | yes | json | JSONField | jsonb/text |
| 21 | `module_view` | BooleanField | `module_view` |  |  |  | bool | BooleanField |  |
| 22 | `cycle_view` | BooleanField | `cycle_view` |  |  |  | bool | BooleanField |  |
| 23 | `issue_views_view` | BooleanField | `issue_views_view` |  |  |  | bool | BooleanField |  |
| 24 | `page_view` | BooleanField | `page_view` |  |  |  | bool | BooleanField |  |
| 25 | `intake_view` | BooleanField | `intake_view` |  |  |  | bool | BooleanField |  |
| 26 | `is_time_tracking_enabled` | BooleanField | `is_time_tracking_enabled` |  |  |  | bool | BooleanField |  |
| 27 | `is_issue_type_enabled` | BooleanField | `is_issue_type_enabled` |  |  |  | bool | BooleanField |  |
| 28 | `guest_view_all_features` | BooleanField | `guest_view_all_features` |  |  |  | bool | BooleanField |  |
| 29 | `cover_image` | CharField | `cover_image` |  |  | yes | str | TextField |  |
| 30 | `archive_in` | IntegerField | `archive_in` |  |  |  | int | IntegerField |  |
| 31 | `close_in` | IntegerField | `close_in` |  |  |  | int | IntegerField |  |
| 32 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 33 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 34 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 35 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 36 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 37 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 38 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 39 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 40 | `default_assignee` | PrimaryKeyRelatedField | `default_assignee` |  |  | yes | uuid | ForeignKey | pk uuid |
| 41 | `project_lead` | PrimaryKeyRelatedField | `project_lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 42 | `cover_image_asset` | PrimaryKeyRelatedField | `cover_image_asset` |  |  | yes | uuid | ForeignKey | pk uuid |
| 43 | `estimate` | PrimaryKeyRelatedField | `estimate` |  |  | yes | uuid | ForeignKey | pk uuid |
| 44 | `default_state` | PrimaryKeyRelatedField | `default_state` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.ProjectLiteSerializer`

Model: `db.Project` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `identifier` | CharField | `identifier` | yes |  |  | str | CharField |  |
| 3 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 4 | `cover_image` | CharField | `cover_image` | yes |  | yes | str | TextField |  |
| 5 | `cover_image_url` | ReadOnlyField | `cover_image_url` | yes |  |  | str | property |  |
| 6 | `logo_props` | JSONField | `logo_props` | yes |  |  | json | JSONField | jsonb/text |
| 7 | `description` | CharField | `description` | yes |  |  | str | TextField |  |

### `plane.app.serializers.project.ProjectMemberAdminSerializer`

Model: `db.ProjectMember` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 3 | `project` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `member` | UserAdminLiteSerializer | `member` | yes |  |  | serializer | ForeignKey | nested UserAdminLiteSerializer |
| 5 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `comment` | CharField | `comment` |  |  | yes | str | TextField |  |
| 9 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 10 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `default_props` | JSONField | `default_props` |  |  |  | json | JSONField | jsonb/text |
| 12 | `preferences` | JSONField | `preferences` |  |  |  | json | JSONField | jsonb/text |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `is_active` | BooleanField | `is_active` |  |  |  | bool | BooleanField |  |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.ProjectMemberInvitePublicSerializer`

Model: `db.ProjectMemberInvite` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 3 | `workspace` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 4 | `role` | ChoiceField | `role` | yes |  |  | int | PositiveSmallIntegerField | 3 choices |
| 5 | `message` | CharField | `message` | yes |  | yes | str | TextField |  |
| 6 | `accepted` | BooleanField | `accepted` | yes |  |  | bool | BooleanField |  |
| 7 | `responded_at` | DateTimeField | `responded_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |

### `plane.app.serializers.project.ProjectMemberInviteSerializer`

Model: `db.ProjectMemberInvite` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 3 | `workspace` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `email` | CharField | `email` |  |  |  | str | CharField |  |
| 8 | `accepted` | BooleanField | `accepted` |  |  |  | bool | BooleanField |  |
| 9 | `token` | CharField | `token` |  |  |  | str | CharField |  |
| 10 | `message` | CharField | `message` |  |  | yes | str | TextField |  |
| 11 | `responded_at` | DateTimeField | `responded_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 12 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 13 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.ProjectMemberLiteSerializer`

Model: `db.ProjectMember` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `member` | UserLiteSerializer | `member` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 2 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 3 | `is_subscribed` | BooleanField | `is_subscribed` | yes |  |  | bool | annotation |  |

### `plane.app.serializers.project.ProjectMemberPreferenceSerializer`

Model: `db.ProjectMember` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `preferences` | JSONField | `preferences` |  |  |  | json | JSONField | jsonb/text |
| 2 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 3 | `member_id` | ReadOnlyField | `member_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `workspace_id` | ReadOnlyField | `workspace_id` | yes |  |  | uuid | ForeignKey |  |

### `plane.app.serializers.project.ProjectMemberRoleSerializer`

Model: `db.ProjectMember` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 3 | `member` | PrimaryKeyRelatedField | `member` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 5 | `original_role` | IntegerField | `role` | yes |  |  | int | PositiveSmallIntegerField |  |
| 6 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |

### `plane.app.serializers.project.ProjectMemberSerializer`

Model: `db.ProjectMember` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 3 | `project` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `member` | UserLiteSerializer | `member` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 5 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `comment` | CharField | `comment` |  |  | yes | str | TextField |  |
| 9 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 10 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `default_props` | JSONField | `default_props` |  |  |  | json | JSONField | jsonb/text |
| 12 | `preferences` | JSONField | `preferences` |  |  |  | json | JSONField | jsonb/text |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `is_active` | BooleanField | `is_active` |  |  |  | bool | BooleanField |  |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.ProjectPublicMemberSerializer`

Model: `db.ProjectPublicMember` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 6 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 8 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `member` | PrimaryKeyRelatedField | `member` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.project.ProjectSerializer`

Model: `db.Project` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 3 | `inbox_view` | BooleanField | `intake_view` | yes |  |  | bool | BooleanField |  |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 8 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 9 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 10 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 11 | `network` | ChoiceField | `network` |  |  |  | int | PositiveSmallIntegerField | 2 choices |
| 12 | `identifier` | CharField | `identifier` |  |  |  | str | CharField |  |
| 13 | `emoji` | CharField | `emoji` |  |  | yes | str | CharField |  |
| 14 | `icon_prop` | JSONField | `icon_prop` |  |  | yes | json | JSONField | jsonb/text |
| 15 | `module_view` | BooleanField | `module_view` |  |  |  | bool | BooleanField |  |
| 16 | `cycle_view` | BooleanField | `cycle_view` |  |  |  | bool | BooleanField |  |
| 17 | `issue_views_view` | BooleanField | `issue_views_view` |  |  |  | bool | BooleanField |  |
| 18 | `page_view` | BooleanField | `page_view` |  |  |  | bool | BooleanField |  |
| 19 | `intake_view` | BooleanField | `intake_view` |  |  |  | bool | BooleanField |  |
| 20 | `is_time_tracking_enabled` | BooleanField | `is_time_tracking_enabled` |  |  |  | bool | BooleanField |  |
| 21 | `is_issue_type_enabled` | BooleanField | `is_issue_type_enabled` |  |  |  | bool | BooleanField |  |
| 22 | `guest_view_all_features` | BooleanField | `guest_view_all_features` |  |  |  | bool | BooleanField |  |
| 23 | `cover_image` | CharField | `cover_image` |  |  | yes | str | TextField |  |
| 24 | `archive_in` | IntegerField | `archive_in` |  |  |  | int | IntegerField |  |
| 25 | `close_in` | IntegerField | `close_in` |  |  |  | int | IntegerField |  |
| 26 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 27 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 28 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 29 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 30 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 31 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 32 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 33 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 34 | `default_assignee` | PrimaryKeyRelatedField | `default_assignee` |  |  | yes | uuid | ForeignKey | pk uuid |
| 35 | `project_lead` | PrimaryKeyRelatedField | `project_lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 36 | `cover_image_asset` | PrimaryKeyRelatedField | `cover_image_asset` |  |  | yes | uuid | ForeignKey | pk uuid |
| 37 | `estimate` | PrimaryKeyRelatedField | `estimate` |  |  | yes | uuid | ForeignKey | pk uuid |
| 38 | `default_state` | PrimaryKeyRelatedField | `default_state` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.state.StateLiteSerializer`

Model: `db.State` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `color` | CharField | `color` | yes |  |  | str | CharField |  |
| 4 | `group` | ChoiceField | `group` | yes |  |  | str | CharField | 6 choices |

### `plane.app.serializers.state.StateSerializer`

Model: `db.State` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 3 | `workspace_id` | ReadOnlyField | `workspace_id` | yes |  |  | uuid | ForeignKey |  |
| 4 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 5 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 6 | `group` | ChoiceField | `group` |  |  |  | str | CharField | 6 choices |
| 7 | `default` | BooleanField | `default` | yes |  |  | bool | BooleanField |  |
| 8 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 9 | `sequence` | FloatField | `sequence` |  |  |  | float | FloatField |  |
| 10 | `order` | FloatField | `order` |  |  |  | float | annotation |  |

### `plane.app.serializers.user.AccountSerializer`

Model: `db.Account` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `provider_account_id` | CharField | `provider_account_id` |  |  |  | str | CharField |  |
| 5 | `provider` | ChoiceField | `provider` |  |  |  | str | CharField | 3 choices |
| 6 | `access_token` | CharField | `access_token` |  |  |  | str | TextField |  |
| 7 | `access_token_expired_at` | DateTimeField | `access_token_expired_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `refresh_token` | CharField | `refresh_token` |  |  | yes | str | TextField |  |
| 9 | `refresh_token_expired_at` | DateTimeField | `refresh_token_expired_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 10 | `last_connected_at` | DateTimeField | `last_connected_at` |  |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `id_token` | CharField | `id_token` |  |  |  | str | TextField |  |
| 12 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 13 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.user.ChangePasswordSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `old_password` | CharField | `old_password` |  |  |  | str |  |  |
| 2 | `new_password` | CharField | `new_password` |  |  |  | str |  |  |
| 3 | `confirm_password` | CharField | `confirm_password` |  |  |  | str |  |  |

### `plane.app.serializers.user.ProfileSerializer`

Model: `db.Profile` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `theme` | JSONField | `theme` |  |  |  | json | JSONField | jsonb/text |
| 5 | `is_app_rail_docked` | BooleanField | `is_app_rail_docked` |  |  |  | bool | BooleanField |  |
| 6 | `is_tour_completed` | BooleanField | `is_tour_completed` |  |  |  | bool | BooleanField |  |
| 7 | `onboarding_step` | JSONField | `onboarding_step` |  |  |  | json | JSONField | jsonb/text |
| 8 | `use_case` | CharField | `use_case` |  |  | yes | str | TextField |  |
| 9 | `role` | CharField | `role` |  |  | yes | str | CharField |  |
| 10 | `is_onboarded` | BooleanField | `is_onboarded` |  |  |  | bool | BooleanField |  |
| 11 | `last_workspace_id` | UUIDField | `last_workspace_id` |  |  | yes | uuid | UUIDField |  |
| 12 | `billing_address_country` | CharField | `billing_address_country` |  |  |  | str | CharField |  |
| 13 | `billing_address` | JSONField | `billing_address` |  |  | yes | json | JSONField | jsonb/text |
| 14 | `has_billing_address` | BooleanField | `has_billing_address` |  |  |  | bool | BooleanField |  |
| 15 | `company_name` | CharField | `company_name` |  |  |  | str | CharField |  |
| 16 | `notification_view_mode` | ChoiceField | `notification_view_mode` |  |  |  | str | CharField | 2 choices |
| 17 | `is_smooth_cursor_enabled` | BooleanField | `is_smooth_cursor_enabled` |  |  |  | bool | BooleanField |  |
| 18 | `is_mobile_onboarded` | BooleanField | `is_mobile_onboarded` |  |  |  | bool | BooleanField |  |
| 19 | `mobile_onboarding_step` | JSONField | `mobile_onboarding_step` |  |  |  | json | JSONField | jsonb/text |
| 20 | `mobile_timezone_auto_set` | BooleanField | `mobile_timezone_auto_set` |  |  |  | bool | BooleanField |  |
| 21 | `language` | CharField | `language` |  |  |  | str | CharField |  |
| 22 | `start_of_the_week` | ChoiceField | `start_of_the_week` |  |  |  | int | PositiveSmallIntegerField | 7 choices |
| 23 | `goals` | JSONField | `goals` |  |  |  | json | JSONField | jsonb/text |
| 24 | `background_color` | CharField | `background_color` |  |  |  | str | CharField |  |
| 25 | `is_navigation_tour_completed` | BooleanField | `is_navigation_tour_completed` |  |  |  | bool | BooleanField |  |
| 26 | `has_marketing_email_consent` | BooleanField | `has_marketing_email_consent` |  |  |  | bool | BooleanField |  |
| 27 | `is_subscribed_to_changelog` | BooleanField | `is_subscribed_to_changelog` |  |  |  | bool | BooleanField |  |
| 28 | `product_tour` | JSONField | `product_tour` |  |  |  | json | JSONField | jsonb/text |
| 29 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | OneToOneField | pk uuid |

### `plane.app.serializers.user.ResetPasswordSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `new_password` | CharField | `new_password` |  |  |  | str |  |  |

### `plane.app.serializers.user.UserAdminLiteSerializer`

Model: `db.User` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `first_name` | CharField | `first_name` |  |  |  | str | CharField |  |
| 3 | `last_name` | CharField | `last_name` |  |  |  | str | CharField |  |
| 4 | `avatar` | CharField | `avatar` |  |  |  | str | TextField |  |
| 5 | `avatar_url` | ReadOnlyField | `avatar_url` | yes |  |  | str | property |  |
| 6 | `is_bot` | BooleanField | `is_bot` | yes |  |  | bool | BooleanField |  |
| 7 | `display_name` | CharField | `display_name` |  |  |  | str | CharField |  |
| 8 | `email` | CharField | `email` |  |  | yes | str | CharField |  |
| 9 | `last_login_medium` | CharField | `last_login_medium` |  |  |  | str | CharField |  |

### `plane.app.serializers.user.UserLiteSerializer`

Model: `db.User` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `first_name` | CharField | `first_name` |  |  |  | str | CharField |  |
| 3 | `last_name` | CharField | `last_name` |  |  |  | str | CharField |  |
| 4 | `avatar` | CharField | `avatar` |  |  |  | str | TextField |  |
| 5 | `avatar_url` | ReadOnlyField | `avatar_url` | yes |  |  | str | property |  |
| 6 | `is_bot` | BooleanField | `is_bot` | yes |  |  | bool | BooleanField |  |
| 7 | `display_name` | CharField | `display_name` |  |  |  | str | CharField |  |

### `plane.app.serializers.user.UserMeSerializer`

Model: `db.User` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `avatar` | CharField | `avatar` | yes |  |  | str | TextField |  |
| 3 | `cover_image` | URLField | `cover_image` | yes |  | yes | str | URLField |  |
| 4 | `avatar_url` | ReadOnlyField | `avatar_url` | yes |  |  | str | property |  |
| 5 | `cover_image_url` | ReadOnlyField | `cover_image_url` | yes |  |  | str | property |  |
| 6 | `date_joined` | DateTimeField | `date_joined` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `display_name` | CharField | `display_name` | yes |  |  | str | CharField |  |
| 8 | `email` | CharField | `email` | yes |  | yes | str | CharField |  |
| 9 | `first_name` | CharField | `first_name` | yes |  |  | str | CharField |  |
| 10 | `last_name` | CharField | `last_name` | yes |  |  | str | CharField |  |
| 11 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 12 | `is_bot` | BooleanField | `is_bot` | yes |  |  | bool | BooleanField |  |
| 13 | `is_email_verified` | BooleanField | `is_email_verified` | yes |  |  | bool | BooleanField |  |
| 14 | `user_timezone` | ChoiceField | `user_timezone` | yes |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 15 | `username` | CharField | `username` | yes |  |  | str | CharField |  |
| 16 | `is_password_autoset` | BooleanField | `is_password_autoset` | yes |  |  | bool | BooleanField |  |
| 17 | `last_login_medium` | CharField | `last_login_medium` | yes |  |  | str | CharField |  |
| 18 | `last_login_time` | DateTimeField | `last_login_time` | yes |  | yes | datetime | DateTimeField | ISO 8601 |

### `plane.app.serializers.user.UserMeSettingsSerializer`

Model: `db.User` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `email` | CharField | `email` | yes |  | yes | str | CharField |  |
| 3 | `workspace` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_workspace |

### `plane.app.serializers.user.UserSerializer`

Model: `db.User` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `last_login` | DateTimeField | `last_login` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 2 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 3 | `username` | CharField | `username` | yes |  |  | str | CharField |  |
| 4 | `mobile_number` | CharField | `mobile_number` | yes |  | yes | str | CharField |  |
| 5 | `email` | CharField | `email` | yes |  | yes | str | CharField |  |
| 6 | `display_name` | CharField | `display_name` |  |  |  | str | CharField |  |
| 7 | `first_name` | CharField | `first_name` |  |  |  | str | CharField |  |
| 8 | `last_name` | CharField | `last_name` |  |  |  | str | CharField |  |
| 9 | `avatar` | CharField | `avatar` |  |  |  | str | TextField |  |
| 10 | `avatar_asset` | PrimaryKeyRelatedField | `avatar_asset` |  |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `cover_image` | URLField | `cover_image` |  |  | yes | str | URLField |  |
| 12 | `cover_image_asset` | PrimaryKeyRelatedField | `cover_image_asset` |  |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `date_joined` | DateTimeField | `date_joined` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 14 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 15 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 16 | `last_location` | CharField | `last_location` | yes |  |  | str | CharField |  |
| 17 | `created_location` | CharField | `created_location` | yes |  |  | str | CharField |  |
| 18 | `is_superuser` | BooleanField | `is_superuser` | yes |  |  | bool | BooleanField |  |
| 19 | `is_managed` | BooleanField | `is_managed` | yes |  |  | bool | BooleanField |  |
| 20 | `is_password_expired` | BooleanField | `is_password_expired` |  |  |  | bool | BooleanField |  |
| 21 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 22 | `is_staff` | BooleanField | `is_staff` | yes |  |  | bool | BooleanField |  |
| 23 | `is_email_verified` | BooleanField | `is_email_verified` | yes |  |  | bool | BooleanField |  |
| 24 | `is_password_autoset` | BooleanField | `is_password_autoset` | yes |  |  | bool | BooleanField |  |
| 25 | `is_password_reset_required` | BooleanField | `is_password_reset_required` |  |  |  | bool | BooleanField |  |
| 26 | `token` | CharField | `token` | yes |  |  | str | CharField |  |
| 27 | `last_active` | DateTimeField | `last_active` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 28 | `last_login_time` | DateTimeField | `last_login_time` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 29 | `last_logout_time` | DateTimeField | `last_logout_time` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 30 | `last_login_ip` | CharField | `last_login_ip` | yes |  |  | str | CharField |  |
| 31 | `last_logout_ip` | CharField | `last_logout_ip` | yes |  |  | str | CharField |  |
| 32 | `last_login_medium` | CharField | `last_login_medium` | yes |  |  | str | CharField |  |
| 33 | `last_login_uagent` | CharField | `last_login_uagent` | yes |  |  | str | TextField |  |
| 34 | `token_updated_at` | DateTimeField | `token_updated_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 35 | `is_bot` | BooleanField | `is_bot` | yes |  |  | bool | BooleanField |  |
| 36 | `bot_type` | CharField | `bot_type` |  |  | yes | str | CharField |  |
| 37 | `user_timezone` | ChoiceField | `user_timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 38 | `is_email_valid` | BooleanField | `is_email_valid` |  |  |  | bool | BooleanField |  |
| 39 | `masked_at` | DateTimeField | `masked_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |

### `plane.app.serializers.view.IssueViewSerializer`

Model: `db.IssueView` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `is_favorite` | BooleanField | `is_favorite` | yes |  |  | bool | annotation |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 7 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 8 | `query` | JSONField | `query` | yes |  |  | json | JSONField | jsonb/text |
| 9 | `filters` | JSONField | `filters` |  |  |  | json | JSONField | jsonb/text |
| 10 | `display_filters` | JSONField | `display_filters` |  |  |  | json | JSONField | jsonb/text |
| 11 | `display_properties` | JSONField | `display_properties` |  |  |  | json | JSONField | jsonb/text |
| 12 | `rich_filters` | JSONField | `rich_filters` |  |  |  | json | JSONField | jsonb/text |
| 13 | `access` | ChoiceField | `access` | yes |  |  | int | PositiveSmallIntegerField | 2 choices |
| 14 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 15 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 16 | `is_locked` | BooleanField | `is_locked` | yes |  |  | bool | BooleanField |  |
| 17 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `owned_by` | PrimaryKeyRelatedField | `owned_by` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.view.ViewIssueListSerializer`

Model: `None` · base: `rest_framework.serializers.Serializer` · codegen: **manual**

Manual because: to_representation defined in plane.app.serializers.view.ViewIssueListSerializer.

### `plane.app.serializers.webhook.WebhookLogSerializer`

Model: `db.WebhookLog` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `webhook` | UUIDField | `webhook` | yes |  |  | uuid | UUIDField |  |
| 6 | `event_type` | CharField | `event_type` |  |  | yes | str | CharField |  |
| 7 | `request_method` | CharField | `request_method` |  |  | yes | str | CharField |  |
| 8 | `request_headers` | CharField | `request_headers` |  |  | yes | str | TextField |  |
| 9 | `request_body` | CharField | `request_body` |  |  | yes | str | TextField |  |
| 10 | `response_status` | CharField | `response_status` |  |  | yes | str | TextField |  |
| 11 | `response_headers` | CharField | `response_headers` |  |  | yes | str | TextField |  |
| 12 | `response_body` | CharField | `response_body` |  |  | yes | str | TextField |  |
| 13 | `retry_count` | IntegerField | `retry_count` |  |  |  | int | PositiveSmallIntegerField |  |
| 14 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.webhook.WebhookSerializer`

Model: `db.Webhook` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.app.serializers.webhook.WebhookSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `url` | URLField | `url` |  |  |  | str | URLField |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `is_active` | BooleanField | `is_active` |  |  |  | bool | BooleanField |  |
| 7 | `secret_key` | CharField | `secret_key` | yes |  |  | str | CharField |  |
| 8 | `project` | BooleanField | `project` |  |  |  | bool | BooleanField |  |
| 9 | `issue` | BooleanField | `issue` |  |  |  | bool | BooleanField |  |
| 10 | `module` | BooleanField | `module` |  |  |  | bool | BooleanField |  |
| 11 | `cycle` | BooleanField | `cycle` |  |  |  | bool | BooleanField |  |
| 12 | `issue_comment` | BooleanField | `issue_comment` |  |  |  | bool | BooleanField |  |
| 13 | `is_internal` | BooleanField | `is_internal` |  |  |  | bool | BooleanField |  |
| 14 | `version` | CharField | `version` |  |  |  | str | CharField |  |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.IssueRecentVisitSerializer`

Model: `db.Issue` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 5 | `assignees` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_assignees |
| 6 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 8 | `project_id` | ReadOnlyField | `project_id` | yes |  |  | uuid | ForeignKey |  |
| 9 | `project_identifier` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_project_identifier |

### `plane.app.serializers.workspace.PageRecentVisitSerializer`

Model: `db.Page` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | TextField |  |
| 3 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 4 | `project_id` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_project_id |
| 5 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |
| 6 | `project_identifier` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_project_identifier |

### `plane.app.serializers.workspace.ProjectRecentVisitSerializer`

Model: `db.Project` · base: `rest_framework.serializers.ModelSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | UUIDField | `id` | yes |  |  | uuid | UUIDField |  |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 4 | `project_members` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_project_members |
| 5 | `identifier` | CharField | `identifier` |  |  |  | str | CharField |  |

### `plane.app.serializers.workspace.StickySerializer`

Model: `db.Sticky` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  | yes | str | TextField |  |
| 6 | `description` | JSONField | `description` |  |  |  | json | JSONField | jsonb/text |
| 7 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 8 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 9 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 10 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `color` | CharField | `color` |  |  | yes | str | CharField |  |
| 12 | `background_color` | CharField | `background_color` |  |  | yes | str | CharField |  |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 17 | `owner` | PrimaryKeyRelatedField | `owner` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkSpaceMemberInvitePublicSerializer`

Model: `db.WorkspaceMemberInvite` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `email` | CharField | `email` | yes |  |  | str | CharField |  |
| 3 | `workspace` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 4 | `role` | ChoiceField | `role` | yes |  |  | int | PositiveSmallIntegerField | 3 choices |
| 5 | `message` | CharField | `message` | yes |  | yes | str | TextField |  |
| 6 | `accepted` | BooleanField | `accepted` | yes |  |  | bool | BooleanField |  |
| 7 | `responded_at` | DateTimeField | `responded_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkSpaceMemberInviteSerializer`

Model: `db.WorkspaceMemberInvite` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 3 | `invite_link` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_invite_link |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `email` | CharField | `email` | yes |  |  | str | CharField |  |
| 8 | `accepted` | BooleanField | `accepted` |  |  |  | bool | BooleanField |  |
| 9 | `token` | CharField | `token` | yes |  |  | str | CharField |  |
| 10 | `message` | CharField | `message` | yes |  | yes | str | TextField |  |
| 11 | `responded_at` | DateTimeField | `responded_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 12 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 13 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkSpaceMemberSerializer`

Model: `db.WorkspaceMember` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `member` | UserLiteSerializer | `member` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 7 | `company_role` | CharField | `company_role` |  |  | yes | str | TextField |  |
| 8 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 9 | `default_props` | JSONField | `default_props` |  |  |  | json | JSONField | jsonb/text |
| 10 | `issue_props` | JSONField | `issue_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 12 | `getting_started_checklist` | JSONField | `getting_started_checklist` |  |  |  | json | JSONField | jsonb/text |
| 13 | `tips` | JSONField | `tips` |  |  |  | json | JSONField | jsonb/text |
| 14 | `explored_features` | JSONField | `explored_features` |  |  |  | json | JSONField | jsonb/text |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkSpaceSerializer`

Model: `db.Workspace` · base: `plane.app.serializers.base.DynamicBaseSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `total_members` | IntegerField | `total_members` | yes |  |  | int | annotation |  |
| 3 | `logo_url` | CharField | `logo_url` | yes |  |  | str | property |  |
| 4 | `role` | IntegerField | `role` | yes |  |  | int | annotation |  |
| 5 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 9 | `logo` | CharField | `logo` |  |  | yes | str | TextField |  |
| 10 | `slug` | SlugField | `slug` |  |  |  | str | SlugField |  |
| 11 | `organization_size` | CharField | `organization_size` |  |  | yes | str | CharField |  |
| 12 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 13 | `background_color` | CharField | `background_color` |  |  |  | str | CharField |  |
| 14 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `logo_asset` | PrimaryKeyRelatedField | `logo_asset` |  |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `owner` | PrimaryKeyRelatedField | `owner` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkspaceHomePreferenceSerializer`

Model: `db.WorkspaceHomePreference` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `key` | CharField | `key` |  |  |  | str | CharField |  |
| 2 | `is_enabled` | BooleanField | `is_enabled` |  |  |  | bool | BooleanField |  |
| 3 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |

### `plane.app.serializers.workspace.WorkspaceLiteSerializer`

Model: `db.Workspace` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 2 | `slug` | SlugField | `slug` | yes |  |  | str | SlugField |  |
| 3 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 4 | `logo_url` | ReadOnlyField | `logo_url` | yes |  |  | str | property |  |

### `plane.app.serializers.workspace.WorkspaceMemberAdminSerializer`

Model: `db.WorkspaceMember` · base: `plane.app.serializers.workspace.WorkSpaceMemberSerializer` · codegen: **dynamic**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `member` | UserAdminLiteSerializer | `member` | yes |  |  | serializer | ForeignKey | nested UserAdminLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 7 | `company_role` | CharField | `company_role` |  |  | yes | str | TextField |  |
| 8 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 9 | `default_props` | JSONField | `default_props` |  |  |  | json | JSONField | jsonb/text |
| 10 | `issue_props` | JSONField | `issue_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 12 | `getting_started_checklist` | JSONField | `getting_started_checklist` |  |  |  | json | JSONField | jsonb/text |
| 13 | `tips` | JSONField | `tips` |  |  |  | json | JSONField | jsonb/text |
| 14 | `explored_features` | JSONField | `explored_features` |  |  |  | json | JSONField | jsonb/text |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkspaceMemberMeSerializer`

Model: `db.WorkspaceMember` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `draft_issue_count` | IntegerField | `draft_issue_count` | yes |  |  | int | annotation |  |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `role` | ChoiceField | `role` |  |  |  | int | PositiveSmallIntegerField | 3 choices |
| 7 | `company_role` | CharField | `company_role` |  |  | yes | str | TextField |  |
| 8 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 9 | `default_props` | JSONField | `default_props` |  |  |  | json | JSONField | jsonb/text |
| 10 | `issue_props` | JSONField | `issue_props` |  |  |  | json | JSONField | jsonb/text |
| 11 | `is_active` | BooleanField | `is_active` |  |  |  | bool | BooleanField |  |
| 12 | `getting_started_checklist` | JSONField | `getting_started_checklist` |  |  |  | json | JSONField | jsonb/text |
| 13 | `tips` | JSONField | `tips` |  |  |  | json | JSONField | jsonb/text |
| 14 | `explored_features` | JSONField | `explored_features` |  |  |  | json | JSONField | jsonb/text |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 18 | `member` | PrimaryKeyRelatedField | `member` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkspaceRecentVisitSerializer`

Model: `db.UserRecentVisit` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `entity_name` | CharField | `entity_name` |  |  |  | str | CharField |  |
| 3 | `entity_identifier` | UUIDField | `entity_identifier` |  |  | yes | uuid | UUIDField |  |
| 4 | `entity_data` | SerializerMethodField | `*` | yes |  |  | method | annotation | method get_entity_data |
| 5 | `visited_at` | DateTimeField | `visited_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |

### `plane.app.serializers.workspace.WorkspaceThemeSerializer`

Model: `db.WorkspaceTheme` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `colors` | JSONField | `colors` |  |  |  | json | JSONField | jsonb/text |
| 7 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `actor` | PrimaryKeyRelatedField | `actor` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkspaceUserLinkSerializer`

Model: `db.WorkspaceUserLink` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 6 | `url` | CharField | `url` |  |  |  | str | TextField |  |
| 7 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `project` | PrimaryKeyRelatedField | `project` |  |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `owner` | PrimaryKeyRelatedField | `owner` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.app.serializers.workspace.WorkspaceUserPreferenceSerializer`

Model: `db.WorkspaceUserPreference` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `key` | CharField | `key` |  |  |  | str | CharField |  |
| 2 | `is_pinned` | BooleanField | `is_pinned` |  |  |  | bool | BooleanField |  |
| 3 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |

### `plane.app.serializers.workspace.WorkspaceUserPropertiesSerializer`

Model: `db.WorkspaceUserProperties` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `filters` | JSONField | `filters` |  |  |  | json | JSONField | jsonb/text |
| 6 | `display_filters` | JSONField | `display_filters` |  |  |  | json | JSONField | jsonb/text |
| 7 | `display_properties` | JSONField | `display_properties` |  |  |  | json | JSONField | jsonb/text |
| 8 | `rich_filters` | JSONField | `rich_filters` |  |  |  | json | JSONField | jsonb/text |
| 9 | `navigation_project_limit` | IntegerField | `navigation_project_limit` |  |  |  | int | IntegerField |  |
| 10 | `navigation_control_preference` | ChoiceField | `navigation_control_preference` |  |  |  | str | CharField | 2 choices |
| 11 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 12 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 14 | `user` | PrimaryKeyRelatedField | `user` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.license.api.serializers.admin.InstanceAdminMeSerializer`

Model: `db.User` · base: `plane.license.api.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `avatar` | CharField | `avatar` | yes |  |  | str | TextField |  |
| 3 | `avatar_url` | ReadOnlyField | `avatar_url` | yes |  |  | str | property |  |
| 4 | `cover_image` | URLField | `cover_image` | yes |  | yes | str | URLField |  |
| 5 | `date_joined` | DateTimeField | `date_joined` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `display_name` | CharField | `display_name` | yes |  |  | str | CharField |  |
| 7 | `email` | CharField | `email` | yes |  | yes | str | CharField |  |
| 8 | `first_name` | CharField | `first_name` | yes |  |  | str | CharField |  |
| 9 | `last_name` | CharField | `last_name` | yes |  |  | str | CharField |  |
| 10 | `is_active` | BooleanField | `is_active` | yes |  |  | bool | BooleanField |  |
| 11 | `is_bot` | BooleanField | `is_bot` | yes |  |  | bool | BooleanField |  |
| 12 | `is_email_verified` | BooleanField | `is_email_verified` | yes |  |  | bool | BooleanField |  |
| 13 | `user_timezone` | ChoiceField | `user_timezone` | yes |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 14 | `username` | CharField | `username` | yes |  |  | str | CharField |  |
| 15 | `is_password_autoset` | BooleanField | `is_password_autoset` | yes |  |  | bool | BooleanField |  |

### `plane.license.api.serializers.admin.InstanceAdminSerializer`

Model: `license.InstanceAdmin` · base: `plane.license.api.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `user_detail` | UserAdminLiteSerializer | `user` | yes |  |  | serializer | ForeignKey | nested UserAdminLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `role` | ChoiceField | `role` |  |  |  | int | PositiveIntegerField | 1 choices |
| 7 | `is_verified` | BooleanField | `is_verified` |  |  |  | bool | BooleanField |  |
| 8 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 9 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `user` | PrimaryKeyRelatedField | `user` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `instance` | PrimaryKeyRelatedField | `instance` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.license.api.serializers.base.BaseSerializer`

Model: `None` · base: `rest_framework.serializers.ModelSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.license.api.serializers.configuration.InstanceConfigurationSerializer`

Model: `license.InstanceConfiguration` · base: `plane.license.api.serializers.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.license.api.serializers.configuration.InstanceConfigurationSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `key` | CharField | `key` |  |  |  | str | CharField |  |
| 6 | `value` | CharField | `value` |  |  | yes | str | TextField |  |
| 7 | `category` | CharField | `category` |  |  |  | str | TextField |  |
| 8 | `is_encrypted` | BooleanField | `is_encrypted` |  |  |  | bool | BooleanField |  |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.license.api.serializers.instance.InstanceSerializer`

Model: `license.Instance` · base: `plane.app.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `primary_owner_details` | UserAdminLiteSerializer | `primary_owner` | yes |  |  | serializer | annotation | nested UserAdminLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `instance_name` | CharField | `instance_name` |  |  |  | str | CharField |  |
| 7 | `whitelist_emails` | CharField | `whitelist_emails` |  |  | yes | str | TextField |  |
| 8 | `instance_id` | CharField | `instance_id` |  |  |  | str | CharField |  |
| 9 | `current_version` | CharField | `current_version` |  |  |  | str | CharField |  |
| 10 | `latest_version` | CharField | `latest_version` |  |  | yes | str | CharField |  |
| 11 | `edition` | CharField | `edition` |  |  |  | str | CharField |  |
| 12 | `domain` | CharField | `domain` |  |  |  | str | TextField |  |
| 13 | `last_checked_at` | DateTimeField | `last_checked_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 14 | `namespace` | CharField | `namespace` |  |  | yes | str | CharField |  |
| 15 | `is_telemetry_enabled` | BooleanField | `is_telemetry_enabled` |  |  |  | bool | BooleanField |  |
| 16 | `is_support_required` | BooleanField | `is_support_required` |  |  |  | bool | BooleanField |  |
| 17 | `is_setup_done` | BooleanField | `is_setup_done` | yes |  |  | bool | BooleanField |  |
| 18 | `is_signup_screen_visited` | BooleanField | `is_signup_screen_visited` |  |  |  | bool | BooleanField |  |
| 19 | `is_verified` | BooleanField | `is_verified` |  |  |  | bool | BooleanField |  |
| 20 | `is_test` | BooleanField | `is_test` |  |  |  | bool | BooleanField |  |
| 21 | `is_current_version_deprecated` | BooleanField | `is_current_version_deprecated` |  |  |  | bool | BooleanField |  |
| 22 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.license.api.serializers.user.UserLiteSerializer`

Model: `db.User` · base: `plane.license.api.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `email` | CharField | `email` |  |  | yes | str | CharField |  |
| 3 | `first_name` | CharField | `first_name` |  |  |  | str | CharField |  |
| 4 | `last_name` | CharField | `last_name` |  |  |  | str | CharField |  |

### `plane.license.api.serializers.workspace.WorkspaceSerializer`

Model: `db.Workspace` · base: `plane.license.api.serializers.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `owner` | UserLiteSerializer | `owner` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `logo_url` | CharField | `logo_url` | yes |  |  | str | property |  |
| 4 | `total_projects` | IntegerField | `total_projects` | yes |  |  | int | annotation |  |
| 5 | `total_members` | IntegerField | `total_members` | yes |  |  | int | annotation |  |
| 6 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 8 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 10 | `logo` | CharField | `logo` |  |  | yes | str | TextField |  |
| 11 | `slug` | SlugField | `slug` |  |  |  | str | SlugField |  |
| 12 | `organization_size` | CharField | `organization_size` |  |  | yes | str | CharField |  |
| 13 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 14 | `background_color` | CharField | `background_color` |  |  |  | str | CharField |  |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `logo_asset` | PrimaryKeyRelatedField | `logo_asset` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.base.BaseSerializer`

Model: `None` · base: `rest_framework.serializers.ModelSerializer` · codegen: **manual**

Manual because: abstract base class: no `Meta.model`, not instantiable.

Abstract base class: no `Meta.model`, not instantiable.

### `plane.space.serializer.cycle.CycleBaseSerializer`

Model: `db.Cycle` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `start_date` | DateTimeField | `start_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 8 | `end_date` | DateTimeField | `end_date` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 10 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 11 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `progress_snapshot` | JSONField | `progress_snapshot` |  |  |  | json | JSONField | jsonb/text |
| 14 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 15 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 16 | `timezone` | ChoiceField | `timezone` |  |  |  | str | CharField | 433 choices (see choice set `choices:adc72be4f903`) |
| 17 | `version` | IntegerField | `version` |  |  |  | int | IntegerField |  |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `owned_by` | PrimaryKeyRelatedField | `owned_by` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.intake.IntakeIssueLiteSerializer`

Model: `db.IntakeIssue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `status` | ChoiceField | `status` | yes |  |  | int | IntegerField | 5 choices |
| 3 | `duplicate_to` | PrimaryKeyRelatedField | `duplicate_to` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 4 | `snoozed_till` | DateTimeField | `snoozed_till` | yes |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `source` | CharField | `source` | yes |  | yes | str | CharField |  |

### `plane.space.serializer.intake.IntakeIssueSerializer`

Model: `db.IntakeIssue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `issue_detail` | IssueFlatSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueFlatSerializer |
| 3 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `status` | ChoiceField | `status` |  |  |  | int | IntegerField | 5 choices |
| 8 | `snoozed_till` | DateTimeField | `snoozed_till` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `source` | CharField | `source` |  |  | yes | str | CharField |  |
| 10 | `source_email` | CharField | `source_email` |  |  | yes | str | TextField |  |
| 11 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `extra` | JSONField | `extra` |  |  |  | json | JSONField | jsonb/text |
| 14 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 17 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 18 | `intake` | PrimaryKeyRelatedField | `intake` |  |  |  | uuid | ForeignKey | pk uuid |
| 19 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 20 | `duplicate_to` | PrimaryKeyRelatedField | `duplicate_to` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.intake.IssueStateIntakeSerializer`

Model: `db.Issue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `state_detail` | StateLiteSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateLiteSerializer |
| 3 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `label_details` | ListSerializer | `labels` | yes |  |  | list<serializer> | ManyToManyField | many nested LabelLiteSerializer |
| 5 | `assignee_details` | ListSerializer | `assignees` | yes |  |  | list<serializer> | ManyToManyField | many nested UserLiteSerializer |
| 6 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 7 | `bridge_id` | UUIDField | `bridge_id` | yes |  |  | uuid | annotation |  |
| 8 | `issue_intake` | ListSerializer | `issue_intake` | yes |  |  | list<serializer> | ManyToOneRel | many nested IntakeIssueLiteSerializer |
| 9 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 11 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 12 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 13 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 14 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 15 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 16 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 17 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 18 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 19 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 20 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 21 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 22 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 23 | `completed_at` | DateTimeField | `completed_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 24 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 25 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 26 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 27 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 28 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 29 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 30 | `project` | PrimaryKeyRelatedField | `project` |  |  |  | uuid | ForeignKey | pk uuid |
| 31 | `workspace` | PrimaryKeyRelatedField | `workspace` |  |  |  | uuid | ForeignKey | pk uuid |
| 32 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 33 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 34 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 35 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 36 | `assignees` | ManyRelatedField | `assignees` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |
| 37 | `labels` | ManyRelatedField | `labels` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.space.serializer.issue.CommentReactionLiteSerializer`

Model: `db.CommentReaction` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `reaction` | CharField | `reaction` |  |  |  | str | TextField |  |
| 3 | `comment` | PrimaryKeyRelatedField | `comment` |  |  |  | uuid | ForeignKey | pk uuid |
| 4 | `actor_detail` | UserLiteSerializer | `actor` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |

### `plane.space.serializer.issue.CommentReactionSerializer`

Model: `db.CommentReaction` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `reaction` | CharField | `reaction` |  |  |  | str | TextField |  |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `actor` | PrimaryKeyRelatedField | `actor` | yes |  |  | uuid | ForeignKey | pk uuid |
| 11 | `comment` | PrimaryKeyRelatedField | `comment` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.IssueAttachmentSerializer`

Model: `db.FileAsset` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `attributes` | JSONField | `attributes` |  |  |  | json | JSONField | jsonb/text |
| 6 | `asset` | FileField | `asset` |  |  |  | str | FileField |  |
| 7 | `entity_type` | CharField | `entity_type` |  |  | yes | str | CharField |  |
| 8 | `entity_identifier` | CharField | `entity_identifier` |  |  | yes | str | CharField |  |
| 9 | `is_deleted` | BooleanField | `is_deleted` |  |  |  | bool | BooleanField |  |
| 10 | `is_archived` | BooleanField | `is_archived` |  |  |  | bool | BooleanField |  |
| 11 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 12 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 13 | `size` | FloatField | `size` |  |  |  | float | FloatField |  |
| 14 | `is_uploaded` | BooleanField | `is_uploaded` |  |  |  | bool | BooleanField |  |
| 15 | `storage_metadata` | JSONField | `storage_metadata` |  |  | yes | json | JSONField | jsonb/text |
| 16 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 18 | `user` | PrimaryKeyRelatedField | `user` |  |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `draft_issue` | PrimaryKeyRelatedField | `draft_issue` |  |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 22 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `comment` | PrimaryKeyRelatedField | `comment` |  |  | yes | uuid | ForeignKey | pk uuid |
| 24 | `page` | PrimaryKeyRelatedField | `page` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.IssueCommentSerializer`

Model: `db.IssueComment` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `actor_detail` | UserLiteSerializer | `actor` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `issue_detail` | IssueFlatSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueFlatSerializer |
| 4 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 5 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 6 | `comment_reactions` | ListSerializer | `comment_reactions` | yes |  |  | list<serializer> | ManyToOneRel | many nested CommentReactionLiteSerializer |
| 7 | `is_member` | BooleanField | `is_member` | yes |  |  | bool | annotation |  |
| 8 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 9 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 10 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 11 | `comment_stripped` | CharField | `comment_stripped` |  |  |  | str | TextField |  |
| 12 | `comment_json` | JSONField | `comment_json` |  |  |  | json | JSONField | jsonb/text |
| 13 | `comment_html` | CharField | `comment_html` |  |  |  | str | TextField |  |
| 14 | `attachments` | ListField | `attachments` |  |  |  | list<str> | ArrayField | child URLField |
| 15 | `access` | ChoiceField | `access` |  |  |  | str | CharField | 2 choices |
| 16 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 17 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 18 | `edited_at` | DateTimeField | `edited_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 19 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 21 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 23 | `description` | PrimaryKeyRelatedField | `description` |  |  | yes | uuid | OneToOneField | pk uuid |
| 24 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 25 | `actor` | PrimaryKeyRelatedField | `actor` |  |  | yes | uuid | ForeignKey | pk uuid |
| 26 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.IssueCreateSerializer`

Model: `db.Issue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **manual**

Manual because: to_representation defined in plane.space.serializer.issue.IssueCreateSerializer.

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `state_detail` | StateSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateSerializer |
| 3 | `created_by_detail` | UserLiteSerializer | `created_by` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 4 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 5 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 6 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 7 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 8 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 9 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 10 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 11 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 12 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 13 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 14 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 15 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 16 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 17 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 18 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 19 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 20 | `completed_at` | DateTimeField | `completed_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 21 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 22 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 23 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 24 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 25 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 26 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 27 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 28 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 29 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 30 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 31 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 32 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |

Write-only fields (not in the response; Phase 5):

| Field | DRF class | Source | Child |
| --- | --- | --- | --- |
| `assignees` | ListField | `assignees` | PrimaryKeyRelatedField |
| `labels` | ListField | `labels` | PrimaryKeyRelatedField |

### `plane.space.serializer.issue.IssueCycleDetailSerializer`

Model: `db.CycleIssue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `cycle_detail` | CycleBaseSerializer | `cycle` | yes |  |  | serializer | ForeignKey | nested CycleBaseSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 11 | `cycle` | PrimaryKeyRelatedField | `cycle` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.IssueFlatSerializer`

Model: `db.Issue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 4 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 5 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 6 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 7 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 8 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 9 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 10 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |

### `plane.space.serializer.issue.IssueLinkSerializer`

Model: `db.IssueLink` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_by_detail` | UserLiteSerializer | `created_by` | yes |  |  | serializer | ForeignKey | nested UserLiteSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `title` | CharField | `title` |  |  | yes | str | CharField |  |
| 7 | `url` | CharField | `url` |  |  |  | str | TextField |  |
| 8 | `metadata` | JSONField | `metadata` |  |  |  | json | JSONField | jsonb/text |
| 9 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 10 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 11 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 12 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 13 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.IssueModuleDetailSerializer`

Model: `db.ModuleIssue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `module_detail` | ModuleBaseSerializer | `module` | yes |  |  | serializer | ForeignKey | nested ModuleBaseSerializer |
| 3 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 6 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 7 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 8 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 9 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 10 | `module` | PrimaryKeyRelatedField | `module` |  |  |  | uuid | ForeignKey | pk uuid |
| 11 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.IssueProjectLiteSerializer`

Model: `db.Issue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 3 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 4 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |

### `plane.space.serializer.issue.IssuePublicSerializer`

Model: `db.Issue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `sequence_id` | IntegerField | `sequence_id` | yes |  |  | int | IntegerField |  |
| 4 | `state` | PrimaryKeyRelatedField | `state` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 5 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 6 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 7 | `priority` | ChoiceField | `priority` | yes |  |  | str | CharField | 5 choices |
| 8 | `target_date` | DateField | `target_date` | yes |  | yes | date | DateField | ISO 8601 |
| 9 | `reactions` | ListSerializer | `issue_reactions` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueReactionSerializer |
| 10 | `votes` | ListSerializer | `votes` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueVoteSerializer |
| 11 | `module_ids` | ListField | `module_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 12 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 13 | `label_ids` | ListField | `label_ids` |  |  |  | list<uuid> | annotation | child UUIDField |
| 14 | `assignee_ids` | ListField | `assignee_ids` |  |  |  | list<uuid> | annotation | child UUIDField |

### `plane.space.serializer.issue.IssueReactionSerializer`

Model: `db.IssueReaction` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 2 | `reaction` | CharField | `reaction` |  |  |  | str | TextField |  |
| 3 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 5 | `actor` | PrimaryKeyRelatedField | `actor` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.IssueRelationSerializer`

Model: `db.IssueRelation` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issue_detail` | IssueProjectLiteSerializer | `related_issue` | yes |  |  | serializer | ForeignKey | nested IssueProjectLiteSerializer |
| 2 | `relation_type` | CharField | `relation_type` |  |  |  | str | CharField |  |
| 3 | `related_issue` | PrimaryKeyRelatedField | `related_issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 4 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 5 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |

### `plane.space.serializer.issue.IssueSerializer`

Model: `db.Issue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 3 | `state_detail` | StateSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateSerializer |
| 4 | `parent_detail` | IssueStateFlatSerializer | `parent` | yes |  |  | serializer | ForeignKey | nested IssueStateFlatSerializer |
| 5 | `label_details` | ListSerializer | `labels` | yes |  |  | list<serializer> | ManyToManyField | many nested LabelSerializer |
| 6 | `assignee_details` | ListSerializer | `assignees` | yes |  |  | list<serializer> | ManyToManyField | many nested UserLiteSerializer |
| 7 | `related_issues` | ListSerializer | `issue_relation` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueRelationSerializer |
| 8 | `issue_relations` | ListSerializer | `issue_related` | yes |  |  | list<serializer> | ManyToOneRel | many nested RelatedIssueSerializer |
| 9 | `issue_cycle` | IssueCycleDetailSerializer | `issue_cycle` | yes |  |  | serializer | ManyToOneRel | nested IssueCycleDetailSerializer |
| 10 | `issue_module` | IssueModuleDetailSerializer | `issue_module` | yes |  |  | serializer | ManyToOneRel | nested IssueModuleDetailSerializer |
| 11 | `issue_link` | ListSerializer | `issue_link` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueLinkSerializer |
| 12 | `issue_attachment` | ListSerializer | `issue_attachment` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueAttachmentSerializer |
| 13 | `sub_issues_count` | IntegerField | `sub_issues_count` | yes |  |  | int | annotation |  |
| 14 | `issue_reactions` | ListSerializer | `issue_reactions` | yes |  |  | list<serializer> | ManyToOneRel | many nested IssueReactionSerializer |
| 15 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 16 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 17 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 18 | `point` | IntegerField | `point` |  |  | yes | int | IntegerField |  |
| 19 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 20 | `description_json` | JSONField | `description_json` |  |  |  | json | JSONField | jsonb/text |
| 21 | `description_html` | CharField | `description_html` |  |  |  | str | TextField |  |
| 22 | `description_stripped` | CharField | `description_stripped` |  |  | yes | str | TextField |  |
| 23 | `description_binary` | ModelField | `description_binary` | yes |  | yes | bytes | BinaryField |  |
| 24 | `priority` | ChoiceField | `priority` |  |  |  | str | CharField | 5 choices |
| 25 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 26 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 27 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 28 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 29 | `completed_at` | DateTimeField | `completed_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 30 | `archived_at` | DateField | `archived_at` |  |  | yes | date | DateField | ISO 8601 |
| 31 | `is_draft` | BooleanField | `is_draft` |  |  |  | bool | BooleanField |  |
| 32 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 33 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 34 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 35 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 36 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 37 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 38 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |
| 39 | `state` | PrimaryKeyRelatedField | `state` |  |  | yes | uuid | ForeignKey | pk uuid |
| 40 | `estimate_point` | PrimaryKeyRelatedField | `estimate_point` |  |  | yes | uuid | ForeignKey | pk uuid |
| 41 | `type` | PrimaryKeyRelatedField | `type` |  |  | yes | uuid | ForeignKey | pk uuid |
| 42 | `assignees` | ManyRelatedField | `assignees` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |
| 43 | `labels` | ManyRelatedField | `labels` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.space.serializer.issue.IssueStateFlatSerializer`

Model: `db.Issue` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `sequence_id` | IntegerField | `sequence_id` |  |  |  | int | IntegerField |  |
| 3 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 4 | `state_detail` | StateLiteSerializer | `state` | yes |  |  | serializer | ForeignKey | nested StateLiteSerializer |
| 5 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |

### `plane.space.serializer.issue.IssueVoteSerializer`

Model: `db.IssueVote` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issue` | PrimaryKeyRelatedField | `issue` | yes |  |  | uuid | ForeignKey | pk uuid |
| 2 | `vote` | ChoiceField | `vote` | yes |  |  | int | IntegerField | 2 choices |
| 3 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 4 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 5 | `actor` | PrimaryKeyRelatedField | `actor` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.LabelLiteSerializer`

Model: `db.Label` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 3 | `color` | CharField | `color` |  |  |  | str | CharField |  |

### `plane.space.serializer.issue.LabelSerializer`

Model: `db.Label` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `workspace_detail` | WorkspaceLiteSerializer | `workspace` | yes |  |  | serializer | ForeignKey | nested WorkspaceLiteSerializer |
| 3 | `project_detail` | ProjectLiteSerializer | `project` | yes |  |  | serializer | ForeignKey | nested ProjectLiteSerializer |
| 4 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 5 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 6 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 7 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 8 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 9 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 10 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 11 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 12 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 13 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 14 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 15 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 16 | `project` | PrimaryKeyRelatedField | `project` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `parent` | PrimaryKeyRelatedField | `parent` |  |  | yes | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.issue.RelatedIssueSerializer`

Model: `db.IssueRelation` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `issue_detail` | IssueProjectLiteSerializer | `issue` | yes |  |  | serializer | ForeignKey | nested IssueProjectLiteSerializer |
| 2 | `relation_type` | CharField | `relation_type` |  |  |  | str | CharField |  |
| 3 | `related_issue` | PrimaryKeyRelatedField | `related_issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 4 | `issue` | PrimaryKeyRelatedField | `issue` |  |  |  | uuid | ForeignKey | pk uuid |
| 5 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |

### `plane.space.serializer.module.ModuleBaseSerializer`

Model: `db.Module` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `description_text` | JSONField | `description_text` |  |  | yes | json | JSONField | jsonb/text |
| 8 | `description_html` | JSONField | `description_html` |  |  | yes | json | JSONField | jsonb/text |
| 9 | `start_date` | DateField | `start_date` |  |  | yes | date | DateField | ISO 8601 |
| 10 | `target_date` | DateField | `target_date` |  |  | yes | date | DateField | ISO 8601 |
| 11 | `status` | ChoiceField | `status` |  |  |  | str | CharField | 6 choices |
| 12 | `view_props` | JSONField | `view_props` |  |  |  | json | JSONField | jsonb/text |
| 13 | `sort_order` | FloatField | `sort_order` |  |  |  | float | FloatField |  |
| 14 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 15 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 16 | `archived_at` | DateTimeField | `archived_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 17 | `logo_props` | JSONField | `logo_props` |  |  |  | json | JSONField | jsonb/text |
| 18 | `created_by` | PrimaryKeyRelatedField | `created_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 19 | `updated_by` | PrimaryKeyRelatedField | `updated_by` | yes |  | yes | uuid | ForeignKey | pk uuid |
| 20 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 21 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |
| 22 | `lead` | PrimaryKeyRelatedField | `lead` |  |  | yes | uuid | ForeignKey | pk uuid |
| 23 | `members` | ManyRelatedField | `members` | yes |  |  | list<uuid> | ManyToManyField | pk uuid; child PrimaryKeyRelatedField |

### `plane.space.serializer.project.ProjectLiteSerializer`

Model: `db.Project` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `identifier` | CharField | `identifier` | yes |  |  | str | CharField |  |
| 3 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 4 | `cover_image` | CharField | `cover_image` | yes |  | yes | str | TextField |  |
| 5 | `icon_prop` | JSONField | `icon_prop` | yes |  | yes | json | JSONField | jsonb/text |
| 6 | `emoji` | CharField | `emoji` | yes |  | yes | str | CharField |  |
| 7 | `description` | CharField | `description` | yes |  |  | str | TextField |  |

### `plane.space.serializer.state.StateLiteSerializer`

Model: `db.State` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 3 | `color` | CharField | `color` | yes |  |  | str | CharField |  |
| 4 | `group` | ChoiceField | `group` | yes |  |  | str | CharField | 6 choices |

### `plane.space.serializer.state.StateSerializer`

Model: `db.State` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `created_at` | DateTimeField | `created_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 3 | `updated_at` | DateTimeField | `updated_at` | yes |  |  | datetime | DateTimeField | ISO 8601 |
| 4 | `deleted_at` | DateTimeField | `deleted_at` |  |  | yes | datetime | DateTimeField | ISO 8601 |
| 5 | `name` | CharField | `name` |  |  |  | str | CharField |  |
| 6 | `description` | CharField | `description` |  |  |  | str | TextField |  |
| 7 | `color` | CharField | `color` |  |  |  | str | CharField |  |
| 8 | `slug` | SlugField | `slug` |  |  |  | str | SlugField |  |
| 9 | `sequence` | FloatField | `sequence` |  |  |  | float | FloatField |  |
| 10 | `group` | ChoiceField | `group` |  |  |  | str | CharField | 6 choices |
| 11 | `is_triage` | BooleanField | `is_triage` |  |  |  | bool | BooleanField |  |
| 12 | `default` | BooleanField | `default` |  |  |  | bool | BooleanField |  |
| 13 | `external_source` | CharField | `external_source` |  |  | yes | str | CharField |  |
| 14 | `external_id` | CharField | `external_id` |  |  | yes | str | CharField |  |
| 15 | `created_by` | PrimaryKeyRelatedField | `created_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 16 | `updated_by` | PrimaryKeyRelatedField | `updated_by` |  |  | yes | uuid | ForeignKey | pk uuid |
| 17 | `project` | PrimaryKeyRelatedField | `project` | yes |  |  | uuid | ForeignKey | pk uuid |
| 18 | `workspace` | PrimaryKeyRelatedField | `workspace` | yes |  |  | uuid | ForeignKey | pk uuid |

### `plane.space.serializer.user.UserLiteSerializer`

Model: `db.User` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |
| 2 | `first_name` | CharField | `first_name` |  |  |  | str | CharField |  |
| 3 | `last_name` | CharField | `last_name` |  |  |  | str | CharField |  |
| 4 | `avatar` | CharField | `avatar` |  |  |  | str | TextField |  |
| 5 | `avatar_url` | ReadOnlyField | `avatar_url` | yes |  |  | str | property |  |
| 6 | `is_bot` | BooleanField | `is_bot` | yes |  |  | bool | BooleanField |  |
| 7 | `display_name` | CharField | `display_name` |  |  |  | str | CharField |  |

### `plane.space.serializer.workspace.WorkspaceLiteSerializer`

Model: `db.Workspace` · base: `plane.space.serializer.base.BaseSerializer` · codegen: **simple**

| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |
| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `name` | CharField | `name` | yes |  |  | str | CharField |  |
| 2 | `slug` | SlugField | `slug` | yes |  |  | str | SlugField |  |
| 3 | `id` | PrimaryKeyRelatedField | `id` | yes |  |  | uuid | UUIDField | pk None |

## View → serializer index

Every unique view in `routes.json`. `serializer_class` is the class attribute; `get_serializer_class` shows the method's file:line and the serializer names it can return; `calls` lists explicit `XSerializer(` calls in the view's own methods (and one level of local helper functions), with `fields=`/`expand=`/`many=` arguments.

| View | Routes | `serializer_class` | `get_serializer_class` | Explicit calls |
| --- | ---: | --- | --- | --- |
| `drf_spectacular.views.SpectacularAPIView` | 1 |  |  |  |
| `drf_spectacular.views.SpectacularRedocView` | 1 |  |  |  |
| `drf_spectacular.views.SpectacularSwaggerView` | 1 |  |  |  |
| `plane.api.views.asset.GenericAssetEndpoint` | 2 |  |  |  |
| `plane.api.views.asset.UserAssetEndpoint` | 2 |  |  |  |
| `plane.api.views.asset.UserServerAssetEndpoint` | 2 |  |  |  |
| `plane.api.views.cycle.CycleArchiveUnarchiveAPIEndpoint` | 3 |  |  | `CycleSerializer` (cycle.py:788, get many=True, fields=self.fields, expand=self.expand) |
| `plane.api.views.cycle.CycleDetailAPIEndpoint` | 1 | `CycleSerializer` |  | `CycleSerializer` (cycle.py:519, get fields=self.fields, expand=self.expand)<br>`CycleSerializer` (cycle.py:551, patch)<br>`CycleUpdateSerializer` (cycle.py:571, patch)<br>`CycleSerializer` (cycle.py:605, patch) |
| `plane.api.views.cycle.CycleIssueDetailAPIEndpoint` | 1 | `CycleIssueSerializer` |  | `CycleIssueSerializer` (cycle.py:1139, get fields=self.fields, expand=self.expand) |
| `plane.api.views.cycle.CycleIssueListCreateAPIEndpoint` | 1 | `CycleIssueSerializer` |  | `IssueSerializer` (cycle.py:943, get many=True, fields=self.fields, expand=self.expand)<br>`CycleIssueSerializer` (cycle.py:1068, post many=True) |
| `plane.api.views.cycle.CycleListCreateAPIEndpoint` | 1 | `CycleSerializer` |  | `CycleSerializer` (cycle.py:205, get many=True, fields=self.fields, expand=self.expand)<br>`CycleSerializer` (cycle.py:220, get many=True, fields=self.fields, expand=self.expand)<br>`CycleSerializer` (cycle.py:235, get many=True, fields=self.fields, expand=self.expand)<br>`CycleSerializer` (cycle.py:250, get many=True, fields=self.fields, expand=self.expand)<br>`CycleSerializer` (cycle.py:265, get many=True, fields=self.fields, expand=self.expand)<br>`CycleSerializer` (cycle.py:276, get many=True, fields=self.fields, expand=self.expand)<br>`CycleCreateSerializer` (cycle.py:310, post)<br>`CycleSerializer` (cycle.py:350, post) |
| `plane.api.views.cycle.CycleListLiteAPIEndpoint` | 1 | `CycleLiteSerializer` |  | `CycleLiteSerializer` (cycle.py:404, get many=True) |
| `plane.api.views.cycle.TransferCycleIssueAPIEndpoint` | 1 |  |  |  |
| `plane.api.views.intake.IntakeIssueDetailAPIEndpoint` | 1 | `IntakeIssueSerializer` |  | `IntakeIssueSerializer` (intake.py:293, get fields=self.fields, expand=self.expand)<br>`IssueSerializer` (intake.py:397, patch)<br>`IntakeIssueUpdateSerializer` (intake.py:409, patch)<br>`IssueSerializer` (intake.py:426, patch)<br>`IntakeIssueSerializer` (intake.py:436, patch)<br>`IntakeIssueSerializer` (intake.py:452, patch)<br>`IntakeIssueSerializer` (intake.py:454, patch) |
| `plane.api.views.intake.IntakeIssueListCreateAPIEndpoint` | 1 | `IntakeIssueSerializer` |  | `IntakeIssueSerializer` (intake.py:123, get many=True, fields=self.fields, expand=self.expand)<br>`IntakeIssueSerializer` (intake.py:229, post) |
| `plane.api.views.invite.WorkspaceInvitationsViewset` | 4 | `WorkspaceInviteSerializer` |  | `WorkspaceInviteSerializer` (invite.py:48, list many=True)<br>`WorkspaceInviteSerializer` (invite.py:57, list many=True)<br>`WorkspaceInviteSerializer` (invite.py:77, retrieve)<br>`WorkspaceInviteSerializer` (invite.py:91, create)<br>`WorkspaceInviteSerializer` (invite.py:119, partial_update) |
| `plane.api.views.issue.IssueActivityDetailAPIEndpoint` | 2 |  |  | `IssueActivitySerializer` (issue.py:1943, get fields=self.fields, expand=self.expand) |
| `plane.api.views.issue.IssueActivityListAPIEndpoint` | 2 |  |  | `IssueActivitySerializer` (issue.py:1878, get many=True, fields=self.fields, expand=self.expand) |
| `plane.api.views.issue.IssueAttachmentDetailAPIEndpoint` | 2 | `IssueAttachmentSerializer` |  | `IssueAttachmentSerializer` (issue.py:2393, patch) |
| `plane.api.views.issue.IssueAttachmentListCreateAPIEndpoint` | 2 | `IssueAttachmentSerializer` |  | `IssueAttachmentSerializer` (issue.py:2132, post)<br>`IssueAttachmentSerializer` (issue.py:2179, get many=True) |
| `plane.api.views.issue.IssueCommentDetailAPIEndpoint` | 2 | `IssueCommentSerializer` |  | `IssueCommentSerializer` (issue.py:1682, get fields=self.fields, expand=self.expand)<br>`IssueCommentSerializer` (issue.py:1729, patch)<br>`IssueCommentCreateSerializer` (issue.py:1750, patch)<br>`IssueCommentSerializer` (issue.py:1774, patch)<br>`IssueCommentSerializer` (issue.py:1811, delete) |
| `plane.api.views.issue.IssueCommentListCreateAPIEndpoint` | 2 | `IssueCommentSerializer` |  | `IssueCommentSerializer` (issue.py:1526, get many=True, fields=self.fields, expand=self.expand)<br>`IssueCommentCreateSerializer` (issue.py:1586, post)<br>`IssueCommentSerializer` (issue.py:1612, post) |
| `plane.api.views.issue.IssueDetailAPIEndpoint` | 2 | `IssueSerializer` |  | `IssueSerializer` (issue.py:671, get fields=self.fields, expand=self.expand)<br>`IssueSerializer` (issue.py:724, put)<br>`IssueSerializer` (issue.py:727, put)<br>`IssueSerializer` (issue.py:775, put)<br>`IssueSerializer` (issue.py:848, patch)<br>`IssueSerializer` (issue.py:849, patch)<br>`IssueSerializer` (issue.py:934, delete) |
| `plane.api.views.issue.IssueLinkDetailAPIEndpoint` | 2 | `IssueLinkSerializer` |  | `IssueLinkSerializer` (issue.py:1359, get fields=self.fields, expand=self.expand)<br>`IssueLinkSerializer` (issue.py:1364, get many=True, fields=self.fields, expand=self.expand)<br>`IssueLinkSerializer` (issue.py:1368, get fields=self.fields, expand=self.expand)<br>`IssueLinkSerializer` (issue.py:1400, patch)<br>`IssueLinkSerializer` (issue.py:1402, patch)<br>`IssueLinkSerializer` (issue.py:1417, patch)<br>`IssueLinkSerializer` (issue.py:1440, delete) |
| `plane.api.views.issue.IssueLinkListCreateAPIEndpoint` | 2 | `IssueLinkSerializer` |  | `IssueLinkSerializer` (issue.py:1250, get many=True, fields=self.fields, expand=self.expand)<br>`IssueLinkCreateSerializer` (issue.py:1281, post)<br>`IssueLinkSerializer` (issue.py:1295, post) |
| `plane.api.views.issue.IssueListCreateAPIEndpoint` | 2 | `IssueSerializer` |  | `IssueSerializer` (issue.py:419, get fields=self.fields, expand=self.expand)<br>`IssueSerializer` (issue.py:512, get many=True, fields=self.fields, expand=self.expand)<br>`IssueSerializer` (issue.py:542, post) |
| `plane.api.views.issue.IssueRelationListCreateAPIEndpoint` | 1 | `IssueRelationSerializer` |  | `IssueRelationSerializer` (issue.py:2689, post many=True)<br>`IssueRelationCreateSerializer` (issue.py:2722, post) |
| `plane.api.views.issue.IssueSearchEndpoint` | 2 |  |  |  |
| `plane.api.views.issue.LabelDetailAPIEndpoint` | 1 | `LabelSerializer` |  | `LabelSerializer` (issue.py:1109, get)<br>`LabelCreateUpdateSerializer` (issue.py:1140, patch)<br>`LabelSerializer` (issue.py:1163, patch) |
| `plane.api.views.issue.LabelListCreateAPIEndpoint` | 1 | `LabelSerializer` |  | `LabelCreateUpdateSerializer` (issue.py:1002, post)<br>`LabelSerializer` (issue.py:1030, post)<br>`LabelSerializer` (issue.py:1076, get many=True, fields=self.fields, expand=self.expand) |
| `plane.api.views.issue.WorkspaceIssueAPIEndpoint` | 2 | `IssueSerializer` |  | `IssueSerializer` (issue.py:326, get fields=self.fields, expand=self.expand) |
| `plane.api.views.member.ProjectMemberDetailAPIEndpoint` | 2 |  |  | `UserLiteSerializer` (member.py:199, get)<br>`ProjectMemberSerializer` (member.py:218, patch) |
| `plane.api.views.member.ProjectMemberListCreateAPIEndpoint` | 2 |  |  | `UserLiteSerializer` (member.py:148, get many=True)<br>`ProjectMemberSerializer` (member.py:161, post) |
| `plane.api.views.member.ProjectMemberLiteAPIEndpoint` | 1 |  |  | `ProjectMemberLiteAPISerializer` (member.py:336, get many=True) |
| `plane.api.views.member.WorkspaceMemberAPIEndpoint` | 1 |  |  | `UserLiteSerializer` (member.py:95, get) |
| `plane.api.views.member.WorkspaceMemberLiteAPIEndpoint` | 1 |  |  | `WorkspaceMemberLiteAPISerializer` (member.py:281, get many=True) |
| `plane.api.views.module.ModuleArchiveUnarchiveAPIEndpoint` | 3 |  |  | `ModuleSerializer` (module.py:1076, get many=True, fields=self.fields, expand=self.expand) |
| `plane.api.views.module.ModuleDetailAPIEndpoint` | 1 | `ModuleSerializer` |  | `ModuleSerializer` (module.py:461, patch)<br>`ModuleUpdateSerializer` (module.py:468, patch)<br>`ModuleSerializer` (module.py:525, get fields=self.fields, expand=self.expand) |
| `plane.api.views.module.ModuleIssueDetailAPIEndpoint` | 1 | `ModuleIssueSerializer` |  | `IssueSerializer` (module.py:909, get many=True, fields=self.fields, expand=self.expand) |
| `plane.api.views.module.ModuleIssueListCreateAPIEndpoint` | 1 | `ModuleIssueSerializer` |  | `IssueSerializer` (module.py:695, get many=True, fields=self.fields, expand=self.expand)<br>`ModuleIssueSerializer` (module.py:788, post many=True) |
| `plane.api.views.module.ModuleListCreateAPIEndpoint` | 1 | `ModuleSerializer` |  | `ModuleCreateSerializer` (module.py:202, post)<br>`ModuleSerializer` (module.py:242, post)<br>`ModuleSerializer` (module.py:276, get many=True, fields=self.fields, expand=self.expand) |
| `plane.api.views.module.ModuleListLiteAPIEndpoint` | 1 | `ModuleLiteSerializer` |  | `ModuleLiteSerializer` (module.py:326, get many=True) |
| `plane.api.views.project.ProjectArchiveUnarchiveAPIEndpoint` | 1 |  |  |  |
| `plane.api.views.project.ProjectDetailAPIEndpoint` | 1 | `ProjectSerializer` |  | `ProjectSerializer` (project.py:526, get fields=self.fields, expand=self.expand)<br>`ProjectSerializer` (project.py:559, patch)<br>`ProjectUpdateSerializer` (project.py:569, patch)<br>`ProjectSerializer` (project.py:599, patch) |
| `plane.api.views.project.ProjectListCreateAPIEndpoint` | 1 | `ProjectSerializer` |  | `ProjectSerializer` (project.py:206, get many=True, fields=self.fields, expand=self.expand)<br>`ProjectCreateSerializer` (project.py:237, post)<br>`ProjectSerializer` (project.py:307, post) |
| `plane.api.views.project.ProjectListLiteAPIEndpoint` | 1 | `ProjectLiteSerializer` |  | `ProjectLiteSerializer` (project.py:430, get many=True) |
| `plane.api.views.project.ProjectSummaryAPIEndpoint` | 1 |  |  |  |
| `plane.api.views.state.StateDetailAPIEndpoint` | 1 | `StateSerializer` |  | `StateSerializer` (state.py:217, get fields=self.fields, expand=self.expand)<br>`StateSerializer` (state.py:290, patch) |
| `plane.api.views.state.StateListCreateAPIEndpoint` | 1 | `StateSerializer` |  | `StateSerializer` (state.py:93, post)<br>`StateSerializer` (state.py:164, get many=True, fields=self.fields, expand=self.expand) |
| `plane.api.views.sticky.StickyViewSet` | 4 | `StickySerializer` |  | `StickySerializer` (sticky.py:50, create)<br>`StickySerializer` (sticky.py:75, list many=True)<br>`StickySerializer` (sticky.py:87, retrieve)<br>`StickySerializer` (sticky.py:98, partial_update) |
| `plane.api.views.user.UserEndpoint` | 1 | `UserLiteSerializer` |  | `UserLiteSerializer` (user.py:40, get) |
| `plane.app.views.analytic.advance.AdvanceAnalyticsChartEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.advance.AdvanceAnalyticsEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.advance.AdvanceAnalyticsStatsEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.base.AnalyticViewViewset` | 2 | `AnalyticViewSerializer` |  |  |
| `plane.app.views.analytic.base.AnalyticsEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.base.DefaultAnalyticsEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.base.ExportAnalyticsEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.base.ProjectStatsEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.base.SavedAnalyticEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsChartEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsEndpoint` | 1 |  |  |  |
| `plane.app.views.analytic.project_analytics.ProjectAdvanceAnalyticsStatsEndpoint` | 1 |  |  |  |
| `plane.app.views.api.ApiTokenEndpoint` | 2 |  |  | `APITokenSerializer` (api.py:37, post)<br>`APITokenReadSerializer` (api.py:44, get many=True)<br>`APITokenReadSerializer` (api.py:48, get)<br>`APITokenSerializer` (api.py:58, patch) |
| `plane.app.views.asset.base.FileAssetEndpoint` | 2 |  |  | `FileAssetSerializer` (base.py:32, get many=True)<br>`FileAssetSerializer` (base.py:44, post) |
| `plane.app.views.asset.base.FileAssetViewSet` | 1 |  |  |  |
| `plane.app.views.asset.base.UserAssetsEndpoint` | 2 |  |  | `FileAssetSerializer` (base.py:90, get)<br>`FileAssetSerializer` (base.py:99, post) |
| `plane.app.views.asset.v2.AssetCheckEndpoint` | 1 |  |  |  |
| `plane.app.views.asset.v2.AssetRestoreEndpoint` | 1 |  |  |  |
| `plane.app.views.asset.v2.DuplicateAssetEndpoint` | 1 |  |  |  |
| `plane.app.views.asset.v2.ProjectAssetDownloadEndpoint` | 1 |  |  |  |
| `plane.app.views.asset.v2.ProjectAssetEndpoint` | 2 |  |  |  |
| `plane.app.views.asset.v2.ProjectBulkAssetEndpoint` | 1 |  |  |  |
| `plane.app.views.asset.v2.StaticFileAssetEndpoint` | 1 |  |  |  |
| `plane.app.views.asset.v2.UserAssetsV2Endpoint` | 2 |  |  |  |
| `plane.app.views.asset.v2.WorkspaceAssetDownloadEndpoint` | 1 |  |  |  |
| `plane.app.views.asset.v2.WorkspaceFileAssetEndpoint` | 2 |  |  |  |
| `plane.app.views.cycle.archive.CycleArchiveUnarchiveEndpoint` | 3 |  |  |  |
| `plane.app.views.cycle.base.CycleAnalyticsEndpoint` | 1 |  |  |  |
| `plane.app.views.cycle.base.CycleDateCheckEndpoint` | 1 |  |  |  |
| `plane.app.views.cycle.base.CycleFavoriteViewSet` | 2 |  |  |  |
| `plane.app.views.cycle.base.CycleProgressEndpoint` | 1 |  |  |  |
| `plane.app.views.cycle.base.CycleUserPropertiesEndpoint` | 1 |  |  | `CycleUserPropertiesSerializer` (base.py:648, patch)<br>`CycleUserPropertiesSerializer` (base.py:659, get) |
| `plane.app.views.cycle.base.CycleViewSet` | 2 | `CycleSerializer` |  | `CycleWriteSerializer` (base.py:276, create)<br>`CycleSerializer` (base.py:346, partial_update)<br>`CycleWriteSerializer` (base.py:360, partial_update) |
| `plane.app.views.cycle.base.TransferCycleIssueEndpoint` | 1 |  |  |  |
| `plane.app.views.cycle.issue.CycleIssueViewSet` | 2 | `CycleIssueSerializer` |  |  |
| `plane.app.views.estimate.base.BulkEstimatePointEndpoint` | 2 | `EstimateSerializer` |  | `EstimateReadSerializer` (base.py:60, list many=True)<br>`EstimatePointSerializer` (base.py:78, create many=True)<br>`EstimateReadSerializer` (base.py:100, create)<br>`EstimateReadSerializer` (base.py:105, retrieve)<br>`EstimateReadSerializer` (base.py:143, partial_update) |
| `plane.app.views.estimate.base.EstimatePointEndpoint` | 2 |  |  | `EstimatePointSerializer` (base.py:178, create)<br>`EstimatePointSerializer` (base.py:190, partial_update)<br>`EstimatePointSerializer` (base.py:266, destroy many=True) |
| `plane.app.views.estimate.base.ProjectEstimatePointEndpoint` | 1 |  |  | `EstimatePointSerializer` (base.py:44, get many=True) |
| `plane.app.views.exporter.base.ExportIssuesEndpoint` | 1 | `ExporterHistorySerializer` |  | `ExporterHistorySerializer` (base.py:82, get many=True) |
| `plane.app.views.external.base.GPTIntegrationEndpoint` | 1 |  |  | `ProjectLiteSerializer` (base.py:177, post)<br>`WorkspaceLiteSerializer` (base.py:178, post) |
| `plane.app.views.external.base.UnsplashEndpoint` | 1 |  |  |  |
| `plane.app.views.external.base.WorkspaceGPTIntegrationEndpoint` | 1 |  |  |  |
| `plane.app.views.intake.base.IntakeIssueViewSet` | 4 | `IntakeIssueSerializer` |  | `IntakeIssueSerializer` (base.py:226, list many=True)<br>`IssueCreateSerializer` (base.py:261, create)<br>`IntakeIssueDetailSerializer` (base.py:330, create)<br>`IssueDetailSerializer` (base.py:412, partial_update)<br>`IssueCreateSerializer` (base.py:414, partial_update)<br>`IntakeIssueSerializer` (base.py:428, partial_update)<br>`IntakeIssueSerializer` (base.py:429, partial_update)<br>`IntakeIssueDetailSerializer` (base.py:504, partial_update)<br>`IntakeIssueDetailSerializer` (base.py:551, retrieve) |
| `plane.app.views.intake.base.IntakeViewSet` | 4 | `IntakeSerializer` |  | `IntakeSerializer` (base.py:77, list) |
| `plane.app.views.intake.base.IntakeWorkItemDescriptionVersionEndpoint` | 2 |  |  | `IssueDescriptionVersionDetailSerializer` (base.py:612, get) |
| `plane.app.views.issue.activity.IssueActivityEndpoint` | 1 |  |  | `IssueActivitySerializer` (activity.py:89, get many=True)<br>`IssueCommentSerializer` (activity.py:93, get many=True)<br>`IssueActivitySerializer` (activity.py:98, get many=True)<br>`IssueCommentSerializer` (activity.py:99, get many=True) |
| `plane.app.views.issue.archive.BulkArchiveIssuesEndpoint` | 1 |  |  | `IssueSerializer` (archive.py:334, post) |
| `plane.app.views.issue.archive.IssueArchiveViewSet` | 2 | `IssueFlatSerializer` |  | `IssueDetailSerializer` (archive.py:253, retrieve expand=self.expand)<br>`IssueSerializer` (archive.py:270, archive)<br>`IssueSerializer` (archive.py:294, unarchive) |
| `plane.app.views.issue.attachment.IssueAttachmentEndpoint` | 2 | `IssueAttachmentSerializer` |  | `IssueAttachmentSerializer` (attachment.py:46, post)<br>`IssueAttachmentSerializer` (attachment.py:112, get many=True) |
| `plane.app.views.issue.attachment.IssueAttachmentV2Endpoint` | 2 | `IssueAttachmentSerializer` |  | `IssueAttachmentSerializer` (attachment.py:174, post)<br>`IssueAttachmentSerializer` (attachment.py:240, get many=True)<br>`IssueAttachmentSerializer` (attachment.py:255, patch) |
| `plane.app.views.issue.base.BulkDeleteIssuesEndpoint` | 1 |  |  |  |
| `plane.app.views.issue.base.DeletedIssuesListViewSet` | 1 |  |  |  |
| `plane.app.views.issue.base.IssueBulkUpdateDateEndpoint` | 1 |  |  |  |
| `plane.app.views.issue.base.IssueDetailEndpoint` | 1 |  |  | `IssueListDetailSerializer` (base.py:1102, get many=True, fields=self.fields, expand=self.expand) |
| `plane.app.views.issue.base.IssueDetailIdentifierEndpoint` | 1 |  |  | `IssueDetailSerializer` (base.py:1368, get expand=self.expand) |
| `plane.app.views.issue.base.IssueListEndpoint` | 1 |  |  | `IssueSerializer` (base.py:174, get many=True, fields=self.fields, expand=self.expand) |
| `plane.app.views.issue.base.IssueMetaEndpoint` | 1 |  |  |  |
| `plane.app.views.issue.base.IssuePaginatedViewSet` | 1 |  |  |  |
| `plane.app.views.issue.base.IssueViewSet` | 2 |  | apps/api/plane/app/views/issue/base.py:216 → `IssueCreateSerializer`, `IssueSerializer` | `IssueCreateSerializer` (base.py:409, create)<br>`IssueDetailSerializer` (base.py:625, retrieve expand=self.expand)<br>`IssueDetailSerializer` (base.py:678, partial_update)<br>`IssueCreateSerializer` (base.py:680, partial_update) |
| `plane.app.views.issue.base.ProjectUserDisplayPropertyEndpoint` | 1 |  |  | `ProjectUserPropertySerializer` (base.py:759, patch)<br>`ProjectUserPropertySerializer` (base.py:771, get) |
| `plane.app.views.issue.comment.CommentReactionViewSet` | 2 | `CommentReactionSerializer` |  | `CommentReactionSerializer` (comment.py:256, create) |
| `plane.app.views.issue.comment.IssueCommentViewSet` | 2 | `IssueCommentSerializer` |  | `IssueCommentSerializer` (comment.py:120, create)<br>`IssueCommentSerializer` (comment.py:154, partial_update)<br>`IssueCommentSerializer` (comment.py:155, partial_update)<br>`IssueCommentSerializer` (comment.py:190, destroy) |
| `plane.app.views.issue.label.BulkCreateIssueLabelsEndpoint` | 1 |  |  | `LabelSerializer` (label.py:115, post many=True) |
| `plane.app.views.issue.label.LabelViewSet` | 2 | `LabelSerializer` |  | `LabelSerializer` (label.py:46, create)<br>`LabelSerializer` (label.py:72, partial_update) |
| `plane.app.views.issue.link.IssueLinkViewSet` | 2 | `IssueLinkSerializer` |  | `IssueLinkSerializer` (link.py:58, create)<br>`IssueLinkSerializer` (link.py:75, create)<br>`IssueLinkSerializer` (link.py:83, partial_update)<br>`IssueLinkSerializer` (link.py:87, partial_update)<br>`IssueLinkSerializer` (link.py:106, partial_update)<br>`IssueLinkSerializer` (link.py:113, destroy) |
| `plane.app.views.issue.reaction.IssueReactionViewSet` | 2 | `IssueReactionSerializer` |  | `IssueReactionSerializer` (reaction.py:60, create) |
| `plane.app.views.issue.relation.IssueRelationViewSet` | 2 | `IssueRelationSerializer` |  | `RelatedIssueSerializer` (relation.py:307, create many=True)<br>`IssueRelationSerializer` (relation.py:312, create many=True)<br>`IssueRelationSerializer` (relation.py:337, remove_relation) |
| `plane.app.views.issue.sub_issue.SubIssuesEndpoint` | 1 |  |  | `IssueSerializer` (sub_issue.py:271, post many=True) |
| `plane.app.views.issue.subscriber.IssueSubscriberViewSet` | 3 | `IssueSubscriberSerializer` |  | `ProjectMemberLiteSerializer` (subscriber.py:77, list many=True)<br>`IssueSubscriberSerializer` (subscriber.py:106, subscribe) |
| `plane.app.views.issue.version.IssueVersionEndpoint` | 2 |  |  | `IssueVersionDetailSerializer` (version.py:50, get) |
| `plane.app.views.issue.version.WorkItemDescriptionVersionEndpoint` | 2 |  |  | `IssueDescriptionVersionDetailSerializer` (version.py:122, get) |
| `plane.app.views.module.archive.ModuleArchiveUnarchiveEndpoint` | 3 |  |  | `ModuleDetailSerializer` (archive.py:318, get) |
| `plane.app.views.module.base.ModuleFavoriteViewSet` | 2 |  |  |  |
| `plane.app.views.module.base.ModuleLinkViewSet` | 2 | `ModuleLinkSerializer` |  |  |
| `plane.app.views.module.base.ModuleUserPropertiesEndpoint` | 1 |  |  | `ModuleUserPropertiesSerializer` (base.py:854, patch)<br>`ModuleUserPropertiesSerializer` (base.py:865, get) |
| `plane.app.views.module.base.ModuleViewSet` | 2 |  | apps/api/plane/app/views/module/base.py:76 → `ModuleSerializer`, `ModuleWriteSerializer` | `ModuleWriteSerializer` (base.py:298, create)<br>`ModuleSerializer` (base.py:358, list many=True, fields=self.fields)<br>`ModuleDetailSerializer` (base.py:425, retrieve)<br>`ModuleSerializer` (base.py:669, partial_update)<br>`ModuleWriteSerializer` (base.py:670, partial_update) |
| `plane.app.views.module.issue.ModuleIssueViewSet` | 3 | `ModuleIssueSerializer` |  |  |
| `plane.app.views.notification.base.MarkAllReadNotificationViewSet` | 1 |  |  |  |
| `plane.app.views.notification.base.NotificationViewSet` | 4 | `NotificationSerializer` |  | `NotificationSerializer` (base.py:150, list many=True)<br>`NotificationSerializer` (base.py:153, list many=True)<br>`NotificationSerializer` (base.py:161, partial_update)<br>`NotificationSerializer` (base.py:173, mark_read)<br>`NotificationSerializer` (base.py:181, mark_unread)<br>`NotificationSerializer` (base.py:189, archive)<br>`NotificationSerializer` (base.py:197, unarchive) |
| `plane.app.views.notification.base.UnreadNotificationEndpoint` | 1 |  |  |  |
| `plane.app.views.notification.base.UserNotificationPreferenceEndpoint` | 1 | `UserNotificationPreferenceSerializer` |  | `UserNotificationPreferenceSerializer` (base.py:303, get)<br>`UserNotificationPreferenceSerializer` (base.py:309, patch) |
| `plane.app.views.page.base.PageDuplicateEndpoint` | 1 |  |  | `PageDetailSerializer` (base.py:635, post) |
| `plane.app.views.page.base.PageFavoriteViewSet` | 1 |  |  |  |
| `plane.app.views.page.base.PageViewSet` | 6 | `PageSerializer` |  | `PageSerializer` (base.py:146, create)<br>`PageDetailSerializer` (base.py:166, create)<br>`PageDetailSerializer` (base.py:198, partial_update)<br>`PageDetailSerializer` (base.py:229, retrieve)<br>`PageSerializer` (base.py:300, list many=True) |
| `plane.app.views.page.base.PagesDescriptionViewSet` | 1 |  |  | `PageBinaryUpdateSerializer` (base.py:552, partial_update) |
| `plane.app.views.page.live.PageLiveAccessEndpoint` | 1 |  |  |  |
| `plane.app.views.page.version.PageVersionEndpoint` | 2 |  |  | `PageVersionDetailSerializer` (version.py:41, get)<br>`PageVersionSerializer` (version.py:52, get many=True) |
| `plane.app.views.project.base.DeployBoardViewSet` | 2 | `DeployBoardSerializer` |  | `DeployBoardSerializer` (base.py:560, list)<br>`DeployBoardSerializer` (base.py:600, create) |
| `plane.app.views.project.base.ProjectArchiveUnarchiveEndpoint` | 1 |  |  |  |
| `plane.app.views.project.base.ProjectFavoritesViewSet` | 2 |  |  |  |
| `plane.app.views.project.base.ProjectIdentifierEndpoint` | 1 |  |  |  |
| `plane.app.views.project.base.ProjectUserViewsEndpoint` | 1 |  |  |  |
| `plane.app.views.project.base.ProjectViewSet` | 3 | `ProjectListSerializer` |  | `ProjectListSerializer` (base.py:140, list_detail many=True)<br>`ProjectListSerializer` (base.py:143, list_detail many=True, fields=fields if fields else None)<br>`ProjectListSerializer` (base.py:255, retrieve)<br>`ProjectSerializer` (base.py:262, create)<br>`ProjectListSerializer` (base.py:311, create)<br>`ProjectSerializer` (base.py:343, partial_update)<br>`ProjectSerializer` (base.py:350, partial_update)<br>`ProjectListSerializer` (base.py:379, partial_update) |
| `plane.app.views.project.invite.ProjectInvitationsViewset` | 2 | `ProjectMemberInviteSerializer` |  |  |
| `plane.app.views.project.invite.ProjectJoinEndpoint` | 1 |  |  | `ProjectMemberInvitePublicSerializer` (invite.py:304, get) |
| `plane.app.views.project.invite.UserProjectInvitationsViewset` | 1 | `ProjectMemberInviteSerializer` |  |  |
| `plane.app.views.project.member.ProjectMemberPreferenceEndpoint` | 1 |  |  | `ProjectMemberPreferenceSerializer` (member.py:426, patch)<br>`ProjectMemberPreferenceSerializer` (member.py:444, get) |
| `plane.app.views.project.member.ProjectMemberUserEndpoint` | 1 |  |  | `ProjectMemberSerializer` (member.py:386, get) |
| `plane.app.views.project.member.ProjectMemberViewSet` | 3 | `ProjectMemberAdminSerializer` |  | `ProjectMemberRoleSerializer` (member.py:162, create many=True)<br>`ProjectMemberRoleSerializer` (member.py:178, list fields=('id', 'member', 'role', 'original_role', 'created_at'), many=True)<br>`ProjectMemberAdminSerializer` (member.py:211, retrieve)<br>`ProjectMemberRoleSerializer` (member.py:213, retrieve fields=('id', 'member', 'role', 'original_role', 'created_at'))<br>`ProjectMemberSerializer` (member.py:307, partial_update) |
| `plane.app.views.project.member.UserProjectRolesEndpoint` | 1 |  |  |  |
| `plane.app.views.search.base.GlobalSearchEndpoint` | 1 |  |  |  |
| `plane.app.views.search.base.SearchEndpoint` | 1 |  |  |  |
| `plane.app.views.search.issue.IssueSearchEndpoint` | 1 |  |  |  |
| `plane.app.views.state.base.IntakeStateEndpoint` | 1 |  |  | `StateSerializer` (base.py:161, get) |
| `plane.app.views.state.base.StateViewSet` | 3 | `StateSerializer` |  | `StateSerializer` (base.py:60, create)<br>`StateSerializer` (base.py:77, partial_update)<br>`StateSerializer` (base.py:92, list many=True) |
| `plane.app.views.timezone.base.TimezoneEndpoint` | 1 |  |  |  |
| `plane.app.views.user.base.AccountEndpoint` | 2 |  |  | `AccountSerializer` (base.py:429, get)<br>`AccountSerializer` (base.py:433, get many=True) |
| `plane.app.views.user.base.ProfileEndpoint` | 1 |  |  | `ProfileSerializer` (base.py:447, get)<br>`ProfileSerializer` (base.py:452, patch) |
| `plane.app.views.user.base.UpdateUserOnBoardedEndpoint` | 1 |  |  |  |
| `plane.app.views.user.base.UpdateUserTourCompletedEndpoint` | 1 |  |  |  |
| `plane.app.views.user.base.UserActivityEndpoint` | 1 |  |  | `IssueActivitySerializer` (base.py:421, get many=True) |
| `plane.app.views.user.base.UserEndpoint` | 5 | `UserSerializer` |  | `UserMeSerializer` (base.py:85, retrieve)<br>`UserMeSettingsSerializer` (base.py:91, retrieve_user_settings)<br>`UserMeSerializer` (base.py:275, update_email) |
| `plane.app.views.user.base.UserSessionEndpoint` | 1 |  |  | `UserMeSerializer` (base.py:383, get) |
| `plane.app.views.view.base.IssueViewFavoriteViewSet` | 2 |  |  |  |
| `plane.app.views.view.base.IssueViewViewSet` | 2 | `IssueViewSerializer` |  | `IssueViewSerializer` (base.py:347, list many=True, fields=fields if fields else None)<br>`IssueViewSerializer` (base.py:375, retrieve)<br>`IssueViewSerializer` (base.py:400, partial_update) |
| `plane.app.views.view.base.WorkspaceViewIssuesViewSet` | 1 |  |  | `ViewIssueListSerializer` (base.py:286, list many=True) |
| `plane.app.views.view.base.WorkspaceViewViewSet` | 2 | `IssueViewSerializer` |  | `IssueViewSerializer` (base.py:90, list many=True, fields=fields if fields else None)<br>`IssueViewSerializer` (base.py:108, partial_update)<br>`IssueViewSerializer` (base.py:139, retrieve) |
| `plane.app.views.webhook.base.WebhookEndpoint` | 2 |  |  | `WebhookSerializer` (base.py:25, post)<br>`WebhookSerializer` (base.py:45, get fields=('id', 'url', 'is_active', 'created_at', 'updated_at', 'project', 'issue', 'cycle', 'module', 'issue_comment'), many=True)<br>`WebhookSerializer` (base.py:64, get fields=('id', 'url', 'is_active', 'created_at', 'updated_at', 'project', 'issue', 'cycle', 'module', 'issue_comment'))<br>`WebhookSerializer` (base.py:84, patch fields=('id', 'url', 'is_active', 'created_at', 'updated_at', 'project', 'issue', 'cycle', 'module', 'issue_comment')) |
| `plane.app.views.webhook.base.WebhookLogsEndpoint` | 1 |  |  | `WebhookLogSerializer` (base.py:128, get many=True) |
| `plane.app.views.webhook.base.WebhookSecretRegenerateEndpoint` | 1 |  |  | `WebhookSerializer` (base.py:120, post) |
| `plane.app.views.workspace.base.ExportWorkspaceUserActivityEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.base.UserWorkSpacesEndpoint` | 1 |  |  | `WorkSpaceSerializer` (base.py:206, get fields=fields if fields else None, many=True) |
| `plane.app.views.workspace.base.UserWorkspaceDashboardEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.base.WorkSpaceAvailabilityCheckEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.base.WorkSpaceViewSet` | 2 | `WorkSpaceSerializer` |  | `WorkSpaceSerializer` (base.py:99, create) |
| `plane.app.views.workspace.base.WorkspaceThemeViewSet` | 2 | `WorkspaceThemeSerializer` |  | `WorkspaceThemeSerializer` (base.py:333, create) |
| `plane.app.views.workspace.cycle.WorkspaceCyclesEndpoint` | 1 |  |  | `CycleSerializer` (cycle.py:108, get many=True) |
| `plane.app.views.workspace.draft.WorkspaceDraftIssueViewSet` | 3 |  |  | `DraftIssueSerializer` (draft.py:129, list many=True)<br>`DraftIssueCreateSerializer` (draft.py:143, create)<br>`DraftIssueCreateSerializer` (draft.py:206, partial_update)<br>`DraftIssueDetailSerializer` (draft.py:233, retrieve)<br>`IssueCreateSerializer` (draft.py:275, create_draft_to_issue) |
| `plane.app.views.workspace.estimate.WorkspaceEstimatesEndpoint` | 1 |  |  | `WorkspaceEstimateSerializer` (estimate.py:32, get many=True) |
| `plane.app.views.workspace.favorite.WorkspaceFavoriteEndpoint` | 2 |  |  | `UserFavoriteSerializer` (favorite.py:35, get many=True)<br>`UserFavoriteSerializer` (favorite.py:54, post)<br>`UserFavoriteSerializer` (favorite.py:58, post)<br>`UserFavoriteSerializer` (favorite.py:83, patch) |
| `plane.app.views.workspace.favorite.WorkspaceFavoriteGroupEndpoint` | 1 |  |  | `UserFavoriteSerializer` (favorite.py:107, get many=True) |
| `plane.app.views.workspace.home.WorkspaceHomePreferenceViewSet` | 2 |  | apps/api/plane/app/views/workspace/home.py:20 → `WorkspaceHomePreferenceSerializer` | `WorkspaceHomePreferenceSerializer` (home.py:72, patch) |
| `plane.app.views.workspace.invite.UserWorkspaceInvitationsViewSet` | 1 | `WorkSpaceMemberInviteSerializer` |  |  |
| `plane.app.views.workspace.invite.WorkspaceInvitationsViewset` | 2 | `WorkSpaceMemberInviteSerializer` |  | `WorkSpaceMemberSerializer` (invite.py:82, create many=True)<br>`WorkSpaceMemberInviteSerializer` (invite.py:139, partial_update) |
| `plane.app.views.workspace.invite.WorkspaceJoinEndpoint` | 1 |  |  | `WorkSpaceMemberInvitePublicSerializer` (invite.py:255, get) |
| `plane.app.views.workspace.label.WorkspaceLabelsEndpoint` | 1 |  |  | `LabelSerializer` (label.py:29, get many=True) |
| `plane.app.views.workspace.member.WorkSpaceMemberViewSet` | 3 | `WorkspaceMemberAdminSerializer` |  | `WorkspaceMemberAdminSerializer` (member.py:72, list fields=NON_GUEST_FIELDS, many=True)<br>`WorkSpaceMemberSerializer` (member.py:74, list fields=('id', 'member', 'role'), many=True)<br>`WorkspaceMemberAdminSerializer` (member.py:91, retrieve fields=NON_GUEST_FIELDS)<br>`WorkSpaceMemberSerializer` (member.py:93, retrieve fields=('id', 'member', 'role'))<br>`WorkSpaceMemberSerializer` (member.py:115, partial_update)<br>`WorkSpaceMemberSerializer` (member.py:139, partial_update) |
| `plane.app.views.workspace.member.WorkspaceMemberUserEndpoint` | 1 |  |  | `WorkspaceMemberMeSerializer` (member.py:250, get) |
| `plane.app.views.workspace.member.WorkspaceMemberUserViewsEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.member.WorkspaceProjectMemberEndpoint` | 1 | `ProjectMemberRoleSerializer` |  | `ProjectMemberRoleSerializer` (member.py:272, get many=True) |
| `plane.app.views.workspace.module.WorkspaceModulesEndpoint` | 1 |  |  | `ModuleSerializer` (module.py:131, get many=True) |
| `plane.app.views.workspace.quick_link.QuickLinkViewSet` | 2 |  | apps/api/plane/app/views/workspace/quick_link.py:20 → `WorkspaceUserLinkSerializer` | `WorkspaceUserLinkSerializer` (quick_link.py:26, create)<br>`WorkspaceUserLinkSerializer` (quick_link.py:38, partial_update)<br>`WorkspaceUserLinkSerializer` (quick_link.py:49, retrieve)<br>`WorkspaceUserLinkSerializer` (quick_link.py:64, list many=True) |
| `plane.app.views.workspace.recent_visit.UserRecentVisitViewSet` | 1 |  | apps/api/plane/app/views/workspace/recent_visit.py:21 → `WorkspaceRecentVisitSerializer` | `WorkspaceRecentVisitSerializer` (recent_visit.py:35, list many=True) |
| `plane.app.views.workspace.state.WorkspaceStatesEndpoint` | 1 |  |  | `StateSerializer` (state.py:40, get many=True) |
| `plane.app.views.workspace.sticky.WorkspaceStickyViewSet` | 2 | `StickySerializer` |  | `StickySerializer` (sticky.py:34, create)<br>`StickySerializer` (sticky.py:50, list many=True) |
| `plane.app.views.workspace.user.UserActivityGraphEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.user.UserIssueCompletedGraphEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.user.UserLastProjectWithWorkspaceEndpoint` | 1 |  |  | `WorkSpaceSerializer` (user.py:98, get)<br>`ProjectMemberSerializer` (user.py:104, get many=True) |
| `plane.app.views.workspace.user.WorkspaceUserActivityEndpoint` | 1 |  |  | `IssueActivitySerializer` (user.py:434, get many=True) |
| `plane.app.views.workspace.user.WorkspaceUserProfileEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.user.WorkspaceUserProfileIssuesEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.user.WorkspaceUserProfileStatsEndpoint` | 1 |  |  |  |
| `plane.app.views.workspace.user.WorkspaceUserPropertiesEndpoint` | 1 |  |  | `WorkspaceUserPropertiesSerializer` (user.py:281, patch)<br>`WorkspaceUserPropertiesSerializer` (user.py:295, get) |
| `plane.app.views.workspace.user_preference.WorkspaceUserPreferenceViewSet` | 1 |  | apps/api/plane/app/views/workspace/user_preference.py:22 → `WorkspaceUserPreferenceSerializer` |  |
| `plane.authentication.views.app.check.EmailCheckEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.email.SignInAuthEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.email.SignUpAuthEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.gitea.GiteaCallbackEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.gitea.GiteaOauthInitiateEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.github.GitHubCallbackEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.github.GitHubOauthInitiateEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.gitlab.GitLabCallbackEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.gitlab.GitLabOauthInitiateEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.google.GoogleCallbackEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.google.GoogleOauthInitiateEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.magic.MagicGenerateEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.magic.MagicSignInEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.magic.MagicSignUpEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.password_management.ForgotPasswordEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.password_management.ResetPasswordEndpoint` | 1 |  |  |  |
| `plane.authentication.views.app.signout.SignOutAuthEndpoint` | 1 |  |  |  |
| `plane.authentication.views.common.CSRFTokenEndpoint` | 1 |  |  |  |
| `plane.authentication.views.common.ChangePasswordEndpoint` | 1 |  |  |  |
| `plane.authentication.views.common.SetUserPasswordEndpoint` | 1 |  |  | `UserSerializer` (common.py:138, post) |
| `plane.authentication.views.space.check.EmailCheckSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.email.SignInAuthSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.email.SignUpAuthSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.gitea.GiteaCallbackSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.gitea.GiteaOauthInitiateSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.github.GitHubCallbackSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.github.GitHubOauthInitiateSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.gitlab.GitLabCallbackSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.gitlab.GitLabOauthInitiateSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.google.GoogleCallbackSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.google.GoogleOauthInitiateSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.magic.MagicGenerateSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.magic.MagicSignInSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.magic.MagicSignUpSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.password_management.ForgotPasswordSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.password_management.ResetPasswordSpaceEndpoint` | 1 |  |  |  |
| `plane.authentication.views.space.signout.SignOutAuthSpaceEndpoint` | 1 |  |  |  |
| `plane.license.api.views.admin.InstanceAdminEndpoint` | 2 |  |  | `InstanceAdminSerializer` (admin.py:69, post)<br>`InstanceAdminSerializer` (admin.py:81, get many=True) |
| `plane.license.api.views.admin.InstanceAdminSignInEndpoint` | 1 |  |  |  |
| `plane.license.api.views.admin.InstanceAdminSignOutEndpoint` | 1 |  |  |  |
| `plane.license.api.views.admin.InstanceAdminSignUpEndpoint` | 1 |  |  |  |
| `plane.license.api.views.admin.InstanceAdminUserMeEndpoint` | 1 |  |  | `InstanceAdminMeSerializer` (admin.py:413, get) |
| `plane.license.api.views.admin.InstanceAdminUserSessionEndpoint` | 1 |  |  | `InstanceAdminMeSerializer` (admin.py:422, get) |
| `plane.license.api.views.configuration.DisableEmailFeatureEndpoint` | 1 |  |  |  |
| `plane.license.api.views.configuration.EmailCredentialCheckEndpoint` | 1 |  |  |  |
| `plane.license.api.views.configuration.InstanceConfigurationEndpoint` | 1 |  |  | `InstanceConfigurationSerializer` (configuration.py:38, get many=True)<br>`InstanceConfigurationSerializer` (configuration.py:58, patch many=True) |
| `plane.license.api.views.instance.InstanceEndpoint` | 1 |  |  | `InstanceSerializer` (instance.py:46, get)<br>`InstanceSerializer` (instance.py:165, patch) |
| `plane.license.api.views.instance.SignUpScreenVisitedEndpoint` | 1 |  |  |  |
| `plane.license.api.views.workspace.InstanceWorkSpaceAvailabilityCheckEndpoint` | 1 |  |  |  |
| `plane.license.api.views.workspace.InstanceWorkSpaceEndpoint` | 1 | `WorkspaceSerializer` |  | `WorkspaceSerializer` (workspace.py:66, get many=True)<br>`WorkspaceSerializer` (workspace.py:73, post) |
| `plane.space.views.asset.AssetRestoreEndpoint` | 1 |  |  |  |
| `plane.space.views.asset.EntityAssetEndpoint` | 2 |  |  |  |
| `plane.space.views.asset.EntityBulkAssetEndpoint` | 1 |  |  |  |
| `plane.space.views.cycle.ProjectCyclesEndpoint` | 1 |  |  |  |
| `plane.space.views.intake.IntakeIssuePublicViewSet` | 3 | `IntakeIssueSerializer` |  | `IssueStateIntakeSerializer` (intake.py:105, list many=True)<br>`IssueStateIntakeSerializer` (intake.py:186, create)<br>`IssueCreateSerializer` (intake.py:225, partial_update)<br>`IssueSerializer` (intake.py:243, partial_update)<br>`IssueStateIntakeSerializer` (intake.py:269, retrieve) |
| `plane.space.views.issue.CommentReactionPublicViewSet` | 2 | `CommentReactionSerializer` |  | `CommentReactionSerializer` (issue.py:532, create) |
| `plane.space.views.issue.IssueCommentPublicViewSet` | 2 | `IssueCommentSerializer` |  | `IssueCommentSerializer` (issue.py:304, create)<br>`IssueCommentSerializer` (issue.py:352, partial_update)<br>`IssueCommentSerializer` (issue.py:353, partial_update)<br>`IssueCommentSerializer` (issue.py:391, destroy) |
| `plane.space.views.issue.IssueReactionPublicViewSet` | 2 | `IssueReactionSerializer` |  | `IssueReactionSerializer` (issue.py:435, create) |
| `plane.space.views.issue.IssueRetrievePublicEndpoint` | 1 |  |  |  |
| `plane.space.views.issue.IssueVotePublicViewSet` | 1 | `IssueVoteSerializer` |  | `IssueVoteSerializer` (issue.py:662, create) |
| `plane.space.views.issue.ProjectIssuesPublicEndpoint` | 1 |  |  |  |
| `plane.space.views.label.ProjectLabelsEndpoint` | 1 |  |  |  |
| `plane.space.views.meta.ProjectMetaDataEndpoint` | 1 |  |  | `ProjectLiteSerializer` (meta.py:31, get) |
| `plane.space.views.module.ProjectModulesEndpoint` | 1 |  |  |  |
| `plane.space.views.project.ProjectDeployBoardPublicSettingsEndpoint` | 1 |  |  | `DeployBoardSerializer` (project.py:24, get) |
| `plane.space.views.project.ProjectMembersEndpoint` | 1 |  |  |  |
| `plane.space.views.project.WorkspaceProjectAnchorEndpoint` | 1 |  |  | `DeployBoardSerializer` (project.py:61, get) |
| `plane.space.views.project.WorkspaceProjectDeployBoardEndpoint` | 1 |  |  |  |
| `plane.space.views.state.ProjectStatesEndpoint` | 1 |  |  |  |
| `plane.web.views.health_check` | 1 |  |  |  |
| `plane.web.views.robots_txt` | 1 |  |  |  |
| `rest_framework.routers.APIRootView` | 2 |  |  |  |
