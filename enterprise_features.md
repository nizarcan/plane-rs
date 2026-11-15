# Paid features

This file lists the features Plane sells in its paid plans (Pro, Business, Enterprise). None of them exist in this repository: it is the Community Edition, and the paid features live in Plane's closed-source Commercial Edition.

It is a companion to `RUST_REWRITE.md`. That brief covers the Community Edition only. This file is the inventory of what would have to be built on top of it to match a paid plan.

## Sources and how far to trust them

Two sources, both snapshots taken on 2026-11-15:

1. **Plane's pricing page** (`https://plane.so/pricing`). The most current. The tables below follow it. It was read through an automated summary, so re-check any row before building against it.
2. **The plan comparison table shipped in this repository** (`apps/web/components/workspace/billing/comparison/plans.tsx`, with prices in `packages/constants/src/payment.ts`). Older, and it disagrees with the pricing page in places. Features that appear only there are listed in "Only in the in-repo table".

Where the two disagree on which plan a feature belongs to, the pricing page is used and the difference is noted.

Neither source describes behaviour in any detail. For that, use Plane's public documentation (`https://docs.plane.so`) and the public API reference (`https://developers.plane.so`).

## Ground rules for building any of this

- **No commercial code as reference.** The Commercial Edition's source, container images and database schema are proprietary. Do not read, decompile or copy from them, including from a running commercial deployment. Build from public documentation and observed product behaviour only.
- **No shared schema to match.** Unlike the Community Edition port, there is no open schema or API contract for these features. Each one needs its own design: tables, endpoints, and the frontend, since the Community Edition frontend has no screens for them.
- **Frontend work is in scope here.** `RUST_REWRITE.md` forbids editing `apps/web`. Paid features cannot be added without doing so. Treat that as a separate track that starts only after the backend cutover.
- **Names.** "Plane", "Plane Pro", "Plane Business" and similar are Plane Software's product names. A fork that ships these features should not present itself under them.
- **Licence.** This fork is AGPL-3.0. Anything added here is too.

## Plans

| Plan | Price (Cloud) | Notes |
| --- | --- | --- |
| Free | $0 | Up to 12 seats on Cloud. This is roughly what the Community Edition provides. |
| Pro | $8 per seat monthly, $6 per seat billed yearly | In-repo figures. |
| Business | $15 per seat monthly, $13 per seat billed yearly | In-repo figures; the same file's description text quotes $12 and $10. |
| Enterprise | Sales-led | |

The in-repo table also has a retired "One" plan ($799 per workspace, marked inactive). The pricing page mentions an "Enterprise Grid" tier as coming soon.

In the tables below, a plan name means "available from this plan up".

## Project and work management

| Feature | From | Detail |
| --- | --- | --- |
| Estimates, advanced | Pro | Free has basic estimates. |
| Views, public and private | Pro | Free has basic views. Includes sharing a view with chosen members. |
| Publish views | Pro | Put a view on the public web. |
| Bulk operations | Pro | All properties. Business adds transfers and conversions. |
| Active cycles | Pro | All running cycles across projects. |
| Cycle manual start and stop | Pro | |
| Cycle progress charts | Pro | |
| Cycle reports | Pro | On-demand reports with permalinks. In-repo table only. |
| Auto-transfer cycle work items | Business | Move incomplete items to the next cycle. The in-repo table puts this in Pro. |
| Auto-schedule cycles | Business | |
| Dependencies in timeline (Gantt) | Pro | Adjust dependent items visually. |
| Work item types | Pro | Custom types with their own properties. |
| Custom properties | Pro | Project-level in Pro and Business. Workspace-level properties and roll-ups in Enterprise (the in-repo table puts workspace-level in Business). |
| Work item templates | Pro | |
| Work item transfers | Pro | Move an item between projects or cycles. In-repo table only. |
| Epics | Pro | Long-running work that groups work items, cycles and modules. In-repo table only. |
| Initiatives | Pro | Roll up several epics. |
| Milestones | Pro | Called "Checkpoints" in the in-repo table. |
| Updates | Pro | Status updates on projects, epics and initiatives. |
| Project overview | Pro | |
| Module overview | Pro | |
| Auto-assignment in modules | Pro | Linear in Pro; round-robin and capacity in Business. In-repo table only. |
| Public, private and secret projects | Pro | |
| Project states | Pro | Projects laid out by state ("State of Projects"). |
| Teamspaces | Pro | Basic in Pro, advanced in Business. Includes cross-project cycles. |
| Dashboards and widgets | Pro | Basic in Pro, advanced in Business. |
| Recurring work items | Business | |
| Project templates | Business | States, workflows, automations and settings saved as a template. |
| Releases | Business | |
| Baselines and deviations | Business | In-repo table only. |
| Scheduled comms | Business | Scheduled reports and notifications to third-party tools. In-repo table only. |
| Custom SLAs | Enterprise | The in-repo table puts this in Business. |

## Time tracking, workflows and automation

| Feature | From | Detail |
| --- | --- | --- |
| Time tracking and worklogs | Pro | Historical worklogs in Pro; approvals added in Business. |
| Workflows and approvals | Business | A single workflow in Business; multiple workflows with approvals in Enterprise. |
| Trigger-and-action automations | Business | The in-repo table puts this in Pro, with limits of 5,000 flows (Pro), 10,000 (Business), unlimited (Enterprise). |
| Decisions and loops in automations | Business | Marked coming soon in the in-repo table. |

## Knowledge management

| Feature | From | Detail |
| --- | --- | --- |
| Wiki | Pro | Workspace-level pages outside any project. |
| Wiki collections | Pro | |
| Real-time collaboration | Pro | The Community Edition already has a live server for project pages; check what the paid tier adds before building. |
| Work item embeds | Pro | |
| Link pages to work items | Pro | |
| Publish pages | Pro | Public pages. |
| Page templates | Pro | |
| Page versions | Pro | 20 versions or 30 days in Pro; 60 versions or 90 days in Business; unlimited in Enterprise. |
| Page exports | Pro | Basic in Pro; queued downloads in Business. PDF and Word-compatible formats. |
| Enhanced search | Pro | |
| Advanced pages analytics | Pro | Views, shares, comments. The in-repo table puts this in Business. |
| Advanced exports | Pro | |
| Nested pages | Business | |
| Shared pages | Business | |
| Page comments | Business | |
| Databases | Enterprise | Databases and formulas inside pages. |

## Intake and customers

| Feature | From | Detail |
| --- | --- | --- |
| Intake forms | Business | Public web forms that create intake items. |
| Intake email | Business | A mailbox address per project. |
| Intake responsibility | Business | Default assignee for accepted intake items. |
| Customers | Business | Customer records linked to work. |

## AI

Cloud only according to the pricing page.

| Feature | From | Detail |
| --- | --- | --- |
| Monthly AI credits | Pro | 500 per seat in Pro, 1,000 per seat in Business, flexible in Enterprise. |
| AI agents | Pro | |
| AI usage dashboard | Free | Advanced version in Enterprise. |
| AI admin controls | Pro | Basic in Pro, advanced in Business. |

## Navigation and search

| Feature | From | Detail |
| --- | --- | --- |
| PQL (Plane Query Language) | Pro | Search with Boolean operators. In-repo table only. |

## Security and access

| Feature | From | Detail |
| --- | --- | --- |
| Seat limit lifted | Pro | Free is capped at 12 seats on Cloud. The in-repo table gives rough self-hosted capacities of about 50 (Free) and 200 (Pro, Business). |
| Guests | Pro | Five per paid member. |
| Role-based access control | Pro | Custom roles. The in-repo table puts full RBAC in Business. |
| Granular access control | Enterprise | |
| SAML | Pro on self-hosted, Business on Cloud | |
| OIDC | Pro on self-hosted, Business on Cloud | |
| LDAP | Enterprise | |
| Group sync | Enterprise | Sync membership from the identity provider. |
| Work item security | Enterprise | Per-item access restrictions. |
| Domain security | Pro | Restrict or allow sign-in domains. In-repo table only. |
| Two-factor authentication and passkeys | Pro | In-repo table only. |
| Password policy | Pro | In-repo table only. |
| Admin interface | Business | Workspace and project settings overview. In-repo table only. |
| Workspace activity logs | Business | In-repo table only. |
| API-enabled audit logs | Business | Marked coming soon in the in-repo table. |

## Importers

The pricing page lists these as free on Cloud. They are still not in this repository: they run in a separate closed-source service.

| Feature | From | Detail |
| --- | --- | --- |
| Jira | Free | Custom properties import needs Pro, per the in-repo table. |
| Linear | Free | |
| Asana | Free | |
| ClickUp | Free | |
| CSV | Free | |
| Workspace members import | Pro | |
| Confluence | Business | |
| Notion | Business | |

## Integrations

| Feature | From | Detail |
| --- | --- | --- |
| GitHub, GitHub Enterprise | Pro | Two-way sync of work items and states. |
| GitLab, GitLab Enterprise | Pro | |
| Slack | Pro | Activity in Slack and slash commands. |
| Sentry | Pro | |
| Draw.io | Pro | |
| Zapier, Zendesk, Freshdesk | Pro | In-repo table only, under a section marked coming soon. |

## Only in the in-repo table

Listed for completeness. These may be renamed, shipped under another name, or dropped.

- Insights (hindsight, on-demand, foresight), Pro, coming soon.
- Custom reports, Business, coming soon.
- Pre-defined work item templates, Pro, coming soon.
- Approvals for workspace, project and work item type, Business, coming soon.
- Private managed deployments, Enterprise, coming soon.
- One-click deployment and marketplace images (DigitalOcean, Heroku, AWS AMI): packaging, not product features.

## Not features to build

These appear in the plan tables but are commercial terms:

- Cloud storage quotas (5 GB Free, 1 TB Pro, 5 TB Business) and upload size limits (5 MB, 100 MB, 200 MB). On self-hosted these are configuration.
- Support channels, support SLAs, migration and implementation services.
- Billing, licence activation and seat management. A fork has no use for these, and the upgrade prompts in the Community Edition frontend that point at them can be removed.

## Suggested order, if any of this is built

Decide what is wanted before building; the full list is far larger than the Community Edition port. A reasonable ranking by value for a small self-hosted instance:

1. Work item types and custom properties. Many other features depend on them.
2. Epics, initiatives and milestones.
3. Time tracking and worklogs.
4. Wiki and nested pages.
5. Dashboards and widgets.
6. Automations and workflows.
7. OIDC and SAML sign-in.
8. Integrations and importers, one at a time.

Everything in the Enterprise column, the AI credit system, and anything marked coming soon can wait.
