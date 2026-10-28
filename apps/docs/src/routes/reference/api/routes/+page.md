Every route `colophon-server` answers, from its router (`crates/colophon-api/src/lib.rs`), each linked to its section; a test fails when the router and this reference disagree. Below, `{P}` stands for `/api/v1/projects/{project}/environments/{environment}`. "Any member" means anyone who belongs to the project; the permission names (`content.update`, …) are the policy's, and the roles in brackets are the built-in roles that hold them ([Roles](/reference/api/roles/#built-in-roles)). A role's permission on content may be narrowed to some fields or to its holder's own entries ([narrowing](/reference/api/roles/#narrowing-a-permission)). The headers every write needs are on [API conventions](/reference/api/).

## Health and sign-in

| Method and path | Who may call it |
|---|---|
| [`GET /livez`](/reference/api/auth/#get-livez) | anyone |
| [`GET /readyz`](/reference/api/auth/#get-readyz) | anyone |
| [`GET /auth/login`](/reference/api/auth/#get-authlogin) | anyone (a browser) |
| [`GET /auth/callback`](/reference/api/auth/#get-authcallback) | the browser that started the sign-in |
| [`POST /auth/logout`](/reference/api/auth/#post-authlogout) | signed in |
| [`GET /me`](/reference/api/auth/#get-me) | signed in |
| [`POST /auth/device`](/reference/api/auth/#post-authdevice) | anyone |
| [`GET /auth/device/{code}`](/reference/api/auth/#get-authdevicecode) | signed in |
| [`POST /auth/device/approve`](/reference/api/auth/#post-authdeviceapprove) | signed in with a browser session |
| [`POST /auth/device/token`](/reference/api/auth/#post-authdevicetoken) | the device holding the secret |
| [`POST /api/v1/setup`](/reference/api/auth/#post-apiv1setup) | signed in, holding the setup code, while the installation has no administrator |

## Projects, members and invitations

| Method and path | Who may call it |
|---|---|
| [`GET /api/v1/projects`](/reference/api/projects/#get-apiv1projects) | signed in |
| [`POST /api/v1/projects`](/reference/api/projects/#post-apiv1projects) | administrators |
| [`GET /api/v1/projects/{project}`](/reference/api/projects/#get-apiv1projectsproject) | any member |
| [`GET /api/v1/projects/{project}/members`](/reference/api/projects/#get-apiv1projectsprojectmembers) | any member |
| [`PUT /api/v1/projects/{project}/members/{identity}`](/reference/api/projects/#put-apiv1projectsprojectmembersidentity) | `members.manage` (owners) |
| [`DELETE /api/v1/projects/{project}/members/{identity}`](/reference/api/projects/#delete-apiv1projectsprojectmembersidentity) | `members.manage` (owners), or yourself to leave |
| [`GET /api/v1/projects/{project}/invitations`](/reference/api/projects/#get-apiv1projectsprojectinvitations) | `members.manage` (owners) |
| [`POST /api/v1/projects/{project}/invitations`](/reference/api/projects/#post-apiv1projectsprojectinvitations) | `members.manage` (owners) |
| [`DELETE /api/v1/projects/{project}/invitations/{invitation}`](/reference/api/projects/#delete-apiv1projectsprojectinvitationsinvitation) | `members.manage` (owners) |
| [`POST /api/v1/invitations/lookup`](/reference/api/projects/#post-apiv1invitationslookup) | signed in, holding the link |
| [`POST /api/v1/invitations/accept`](/reference/api/projects/#post-apiv1invitationsaccept) | signed in, holding the link |

## Roles

| Method and path | Who may call it |
|---|---|
| [`GET /api/v1/projects/{project}/roles`](/reference/api/roles/#get-apiv1projectsprojectroles) | any member |
| [`POST /api/v1/projects/{project}/roles`](/reference/api/roles/#post-apiv1projectsprojectroles) | `roles.manage` (owners) |
| [`GET /api/v1/projects/{project}/roles/{role}`](/reference/api/roles/#get-apiv1projectsprojectrolesrole) | any member |
| [`PUT /api/v1/projects/{project}/roles/{role}`](/reference/api/roles/#put-apiv1projectsprojectrolesrole) | `roles.manage` (owners) |
| [`DELETE /api/v1/projects/{project}/roles/{role}`](/reference/api/roles/#delete-apiv1projectsprojectrolesrole) | `roles.manage` (owners) |

## Content types

| Method and path | Who may call it |
|---|---|
| [`GET {P}/content-types`](/reference/api/content-types/#get-pcontent-types) | any member |
| [`POST {P}/content-types`](/reference/api/content-types/#post-pcontent-types) | `schema.change` (owners, developers) |
| [`GET {P}/content-types/{slug}`](/reference/api/content-types/#get-pcontent-typesslug) | any member |
| [`PUT {P}/content-types/{slug}`](/reference/api/content-types/#put-pcontent-typesslug) | `schema.change` (owners, developers) |

## Entries and versions

| Method and path | Who may call it |
|---|---|
| [`GET {P}/collections/{collection}/entries`](/reference/api/entries/#get-pcollectionscollectionentries) | `content.read` (every built-in role); only your own entries when that is all your role may read |
| [`POST {P}/collections/{collection}/entries`](/reference/api/entries/#post-pcollectionscollectionentries) | `content.create` (every built-in role) |
| [`GET {P}/collections/{collection}/entries/{entry}`](/reference/api/entries/#get-pcollectionscollectionentriesentry) | `content.read` |
| [`PUT {P}/collections/{collection}/entries/{entry}`](/reference/api/entries/#put-pcollectionscollectionentriesentry) | `content.update` (every built-in role) |
| [`GET {P}/collections/{collection}/entries/{entry}/revisions`](/reference/api/entries/#get-pcollectionscollectionentriesentryrevisions) | `content.read` |
| [`GET {P}/collections/{collection}/entries/{entry}/revisions/{revision}`](/reference/api/entries/#get-pcollectionscollectionentriesentryrevisionsrevision) | `content.read` |
| [`POST /api/v1/rich-text/validate`](/reference/api/entries/#post-apiv1rich-textvalidate) | anyone; no sign-in |

## Publish requests and reviews

| Method and path | Who may call it |
|---|---|
| [`GET {P}/changesets`](/reference/api/publish-requests/#get-pchangesets) | any member |
| [`POST {P}/changesets`](/reference/api/publish-requests/#post-pchangesets) | `content.update` in each collection (every built-in role) |
| [`GET {P}/changesets/{changeset}`](/reference/api/publish-requests/#get-pchangesetschangeset) | any member |
| [`POST {P}/changesets/{changeset}/submit`](/reference/api/publish-requests/#post-pchangesetschangesetsubmit) | `content.update` in each collection |
| [`POST {P}/changesets/{changeset}/refresh`](/reference/api/publish-requests/#post-pchangesetschangesetrefresh) | `content.update` in each collection |
| [`POST {P}/changesets/{changeset}/reviews`](/reference/api/publish-requests/#post-pchangesetschangesetreviews) | `content.review` in each collection (owners, publishers) |
| [`POST {P}/changesets/{changeset}/publish`](/reference/api/publish-requests/#post-pchangesetschangesetpublish) | `content.publish` in each collection (owners, publishers) |

## Schedules

| Method and path | Who may call it |
|---|---|
| [`POST {P}/changesets/{changeset}/schedule`](/reference/api/publish-requests/#post-pchangesetschangesetschedule) | `content.publish` in each collection (owners, publishers) |
| [`GET {P}/schedules`](/reference/api/publish-requests/#get-pschedules) | any member |
| [`DELETE {P}/schedules/{schedule}`](/reference/api/publish-requests/#delete-pschedulesschedule) | whoever scheduled it, or `content.publish` in each collection |

## Releases and receipts

| Method and path | Who may call it |
|---|---|
| [`GET {P}/releases`](/reference/api/releases/#get-preleases) | any member |
| [`GET {P}/releases/{generation}`](/reference/api/releases/#get-preleasesgeneration) | any member |

## Website connections and jobs

| Method and path | Who may call it |
|---|---|
| [`GET {P}/destinations`](/reference/api/website-connections/#get-pdestinations) | `destinations.manage` (owners, developers) |
| [`POST {P}/destinations`](/reference/api/website-connections/#post-pdestinations) | `destinations.manage` (owners, developers) |
| [`PUT {P}/destinations/{destination}`](/reference/api/website-connections/#put-pdestinationsdestination) | `destinations.manage` (owners, developers) |
| [`GET {P}/jobs`](/reference/api/website-connections/#get-pjobs) | any member |

## Public delivery

| Method and path | Who may call it |
|---|---|
| [`GET /content/v1/{project}/{environment}/{collection}`](/reference/delivery-api/#get-contentv1projectenvironmentcollection) | anyone; no sign-in |
| [`GET /content/v1/{project}/{environment}/{collection}/{entry}`](/reference/delivery-api/#get-contentv1projectenvironmentcollectionentry) | anyone; no sign-in |

## Outside the router

With `COLOPHON_STUDIO_DIR` set, the server also mounts the Studio's files at `/studio/` (`crates/colophon-api/src/studio.rs`, added by `colophon-server`). Any path no route matches answers `404` `route_not_found`; a path that exists for other methods answers `405` `method_not_allowed` with an `Allow` header.
