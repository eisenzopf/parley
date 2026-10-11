# Conference implementation evidence

Current voice-only rehearsal: [steps and operator setup](CONFERENCE_VOICE_REHEARSAL.md).
Voice-only evidence below remains historical. The approved SMS flow uses reviewed public-form enrollments.

## Approved SMS integration — October 10, 2026

Telnyx reports the real campaign as `MNO_PROVISIONED`. The sender assignment was
completed as `ASSIGNED`, and the US STOP, HELP and START/UNSTOP rules match the
campaign's exact approved text. The read-only provider configuration check passes.
Real traffic results must be checked independently; configuration does not prove delivery.

The user explicitly confirmed public-form enrollment for the owner and current
organizer stand-in. Only these two mobile endpoints are activated for SMS; Alex
and the Thelve booker use chat for final updates. The booker's Thelve voice route
and the organizer's latest stand-in voice route are preserved. No enrollment is
inferred from the old roster, and the obsolete booker SMS route is not reused for
the organizer. Private reviewed records preserve the actual user confirmation
as evidence, without claiming direct access to the signup database.

Live admission now checks reviewed enrollment and approved branding before
queueing and before sending. The full-mode launcher also checks provider
assignment, keyword rules and server-reviewed routes. The assistant still uses
UCTP alone for communications. Approval produces four individual final updates,
using SMS where enrolled and chat elsewhere; the stage distinguishes chat
acceptance from provider sent and delivered states. The owner must press Start
before any live coordination begins. Live delivery and attributed human replies
remain unverified until that run produces actual signed events.

Validation: 68 ordinary Rust tests, 60 JavaScript checks, three kit-boundary tests,
the browser suite and the two-SMS/two-chat browser case pass. The complete local
SIP/browser/phone-handoff scenario also passes with the new branded final SMS.
The pinned dependency fingerprints and staged secret scan pass. Deployment checks and fresh idle worker status are kept in private evidence.

## Human browser and phone handoff — October 6, 2026, 9:55 PM

Jonathan reports that the fresh run all seemed to work. The journal for
`conv_97ace3cfddb54096b163ad0712437b71` confirms one organizer tool intent
(#1046), browser speaking (#1057), and a press-1-confirmed telephone speaking
move (#1063). Both replacements retain organizer Connection
`conn_1440b26eb6d74c87948bdb6d3c0289d8` in Session
`sess_cf6ed9c142ac46a886e34e226512d80c`. The booker's actual offer was
American Airlines 2263, Reno 11 AM to Atlanta 4 PM, Terminal 2, for two travelers.

Jonathan then hung up the mobile handset. The phone speaking interval in the
journal is 17.80 seconds, so this does not establish the 70-second hold gate.
The journal labels the phone terminal event failed (#1064), then records the
organizer ending normally (#1065) and the Session ending (#1066). The planner
reported a failed call and did not propose final approval. The hangup
classification needs investigation; the handoff's success does not qualify that
ending or the complete approval flow. Browser End voice also terminates this
same Session after the phone move. Real SMS remains deferred. Private evidence:
`var/conference/live/browser-tool-human-completed-20261007-evidence.json` and
`var/conference/live/browser-tool-human-outcome-20261007.json`.

## Organizer browser invitation failure — October 6, 2026, 8:39 PM

The owner started `conv_f3a1f0fcbfbe4b469dae1fff067f799b`. The real booker
supplied American Airlines 2256, Reno 11 AM to Atlanta 4 PM, Terminal 2, for
Jonathan and Alex. David completed his readback/goodbye, ended that Session and
called the consenting organizer stand-in. The organizer confirmed pickup at
the curb. The owner heard David announce Jonathan's handoff, but no browser
invitation was accepted. The owner then hung up; the telephone did not end by
itself. Four subsequent Vapi planning requests timed out at 20 seconds and the
worker exited. Private journal export:
`var/conference/live/organizer-no-browser-invitation-20261007-evidence.json`.

Read-only replay of the pre-hangup snapshot produced request_browser_join in
2.96 seconds, without executing it. This shows the decision is valid, but does
not reproduce or resolve the live timeout. Vapi's saved transcript also lacks
the full announcement heard by the owner; its message timestamps precede some
journal receipt times substantially. The source of that lag is unproven.

The replacement path is implemented and deployed: the live organizer voice model invokes
one explicit zero-argument request_browser_join tool. The host records its
attributed intent in the same UCTP journal, and the external worker sends the
existing browser invitation without a second model request. It waits through
speech, validates the organizer reply/live Session, retains exact command IDs
across ambiguous retries and rings only once. The original organizer Connection
is retained; no new Rvoip patch is involved. The worker/planner suite passes 26 tests; the provider tool parser also passes.
The complete fixture rehearsal passes in 37.65 seconds with one actual tool
event and one browser invitation, five planning requests, real SIP/WebRTC and
confirmed browser-to-phone audio, zero SMS. This uses fixture voice participants.
The hosted native release `20261007-02864f22-browser-tool` is deployed after
its idle-state gate. Its binary SHA-256 is
`126083debefd031e8b7231a3dba56a788ee1dffb34c20f3acff80a774615e6ee`.
HTTPS health and authenticated UCTP preflight pass. A real Vapi synthetic
organizer probe on AWS confirms the model invokes the zero-argument tool.
A second 64.33-second AWS probe captures exactly one real `tool-calls` event
over its live audio WebSocket, with an empty arguments object accepted by the
parser. Both synthetic provider calls are verified ended; neither uses PSTN
or SMS. Private evidence: `var/conference/live/browser-tool-wire-evidence-20261007.json`.
All 55 JavaScript checks
pass. The complete local scenario with fixture SMS now also passes in 48.32
seconds: one tool intent, one browser invitation, retained organizer Connection,
real SIP/WebRTC and browser-to-phone audio, organizer text/reply and four separate
approved final updates. Its deliberately lost worker snapshot response recovers;
the test observer uses an independent owner connection during that recovery.
Private labeled trace:
`test-results/conference-scenario-conv_3f240651f8db4dda807350302a6a8a19-evidence.json`.
Fixture acceptance does not prove carrier delivery. Full human voice qualification
remains pending. Real SMS is deferred.

A new Conversation, `conv_97ace3cfddb54096b163ad0712437b71`, is provisioned
with no SMS endpoints, fresh scoped credentials and one idle worker. The owner
page is connected, displays only conversation.opened #951 and enables Start
coordinating. Jonathan controls Start; no calls are initiated during setup.
The roster uses the Thelve travel booker, the consenting organizer test stand-in
and Jonathan's separate callback handset.

## Fresh full voice rehearsal — October 6, 2026, 8:26 PM

After the reset and retry-context changes, the full voice-only scenario passes
in 49.29 seconds with actual Vapi planning, fixture voice participants and real
local SIP/WebRTC media. Six decisions cover booker invite/end, organizer invite,
browser invitation, proposal and approved voice completion. Both browser and
phone audio are measured, the organizer Connection is retained, callback
cancellation and a second confirmed attempt work, and no SMS updates are sent.
This qualifies the current planning path with synthetic participants, not live
Thelve/PSTN reachability or the complete hosted human story. Private validation
log: `var/conference/live/fresh-voice-live-planner-validation-20261007.log`.

At the owner's request, the continuation worker stopped and the earlier human
Conversation was closed through UCTP with its history preserved. A new voice-only
Conversation, `conv_26778b29b6a84cd3961c2aeddc319d54`, has fresh scoped credentials
and worker state. The roster retains the Thelve booker, the newly selected
organizer test number and Jonathan's callback route, with no SMS endpoints.
Configuration preflight passes with zero active Sessions and zero unsettled
SMS. The owner page visibly connects, shows only conversation.opened #772,
and enables Start coordinating with the original complete trip task. A single
worker is waiting with no pending effects. The owner controls Start; no calls
have been placed in this fresh run. This supersedes the retry-draft setup below.

## Organizer retry — October 6, 2026, 8 PM

The retained human Conversation routes the organizer to the user's new test
number. Owner requests #768 and #769 resent the original trip task. No new
organizer call followed: the Vapi planner reported the historical handoff
failure and waited. Prompt clarification and explicit latest-owner/failure
context pass 25 worker/planner tests, but read-only live planning still waited
on those original task messages; this is not a verified automatic retry fix.
The owner page now contains an explicit organizer retry draft, not submitted by
the operator. A single external voice-only worker is running and waiting for
that follow-up. The existing booker facts and journal are preserved; SMS stays
deferred. The next human organizer/browser/phone rehearsal remains pending.

## Earlier deployed voice releases — October 6, 2026, evening

The preceding release was `20261007-285b16dd-ringtone-control`: the following
role release plus one static ringtone asset. Its native binary is reused
unchanged. The new asset SHA-256 is
`285b16dd6d54900cc30e033dd511982fa83eaf1fccf6bdfb16f48179a073cc09`.
Activation passes the idle-state gate and service health checks; all five public
stage asset hashes match the workspace. Parent release files are preserved.
Ringtone audio resume is now requested without awaiting it, so unavailable
sound cannot indefinitely block Connect or submitting the owner's task.
Three browser regressions pass, including a never-resolving audio resume with
successful connection, visible invitation and exactly one owner submission
after the button is pressed. The ringtone burst/answer unit test also passes.

`20261007-d764aa66-david-roles` is built and activated after the idle-state
gate. Its source archive SHA-256 is
`d764aa665ce41562bb7ac0dc3c823b7f3a4a99c515950b77204eb4773385cdac`;
the installed binary SHA-256 is
`82d97b72a57fa637717a9bbbdb53afda783930ffd1a4b551232d2d21428cf786`.
Both services and health checks pass. Authenticated human preflight has zero
active Sessions, zero unsettled SMS and no configuration problems. It advertises
the nine-file Rvoip diagnostic patch, voice, browser handoff and phone handoff.
The role-specific prompts are now deployed: David asks the booker for inventory
and confirms it, while the organizer receives the saved itinerary and can ask
Jonathan a question. The preceding release has passed real Vapi/SIP-to-WebRTC
takeover. The new release holds the bridge for 114.33 seconds, but that run's
browser stage loses its invitation when the synthetic peer reaches its own
120-second call deadline. That peer's provider call is verified ended. The
test peer now allows six minutes of active call time with a seven-minute
process guard; the corrected binary builds successfully. A fresh cloud run
holds the bridge for 65.80 seconds, then fails before the browser connects.
Its provider call is verified ended. The Mac is locked and native browser
controls cannot continue; the user has been asked to unlock it. The blocking
audio-resume regression was reproduced independently and fixed as above;
the live failure's precise cause has not been instrumented. A fresh run after
the static fix now passes the current-release transport check: the actual
Vapi/SIP bridge holds for 67.37 seconds, browser takeover takes 3.93 seconds,
and two-way Opus audio follows within 227 milliseconds. The original SIP
Connection is retained. Vapi is verified ended 503 milliseconds after takeover,
before the retained SIP Session ends. The synthetic peer ends with 4,053
received frames and 241 browser-tone frames. This uses synthetic SIP people
and no PSTN calls or SMS; the complete human story remains open. No production
timeout policy was changed.

The human worker remains stopped. The saved booker itinerary and Conversation
are retained. The presenter's browser now visibly shows **Connected · Voice
rehearsal · SMS deferred**; the private token copy matches the existing scoped
credentials.
A private operator gate is armed for twenty minutes and resumes the worker
only after a new owner chat request, preserving the presenter's control of the
page's Start button. The voice rehearsal is ready for the presenter's request.
That human rehearsal will qualify actual browser and phone audio on the current
release; full demo qualification remains incomplete.
SMS is deferred another day by the user; no SMS endpoints are in this rehearsal.

## Audio transport isolation — October 6, 2026

Two synthetic diagnostics bypassed the SIP/media bridge: Node's direct
WebSocket and a raw Rust client using the same locked Tokio, Rustls and
Tokio-Tungstenite versions as Parley. Both streamed the full booker speech
and ran for about a minute without an audio-write backlog or deadline failure.
Both provider calls are authoritatively verified ended. These isolated results
narrow the investigation; they do not qualify the complete conference flow
or exclude intermittent provider/network failures.

The following complete Vapi rehearsal fails in 75.67 seconds during the booker
call. Aggregate writer diagnostics show 640-byte frames at approximately
50 messages per second, with an empty queue before writes stall. The first
200-millisecond media deadline expires about 12.9 seconds after the socket
writer starts; its queue subsequently fills. A heartbeat control write then
exceeds its ten-second deadline. The barge-in graph flush completes in less
than one millisecond, and inbound audio continues: 1,489 inbound frames were
released by teardown. The provider's single call is confirmed ended with
`phone-call-provider-closed-websocket`. The organizer and handoff are not reached.

[PR #263](https://github.com/eisenzopf/rvoip/pull/263) now includes safe
traffic/queue counters and barge-in flush timings at commit
`1f80f3e36206b8624675a6e1317e98848741c30e`. All 49 ordinary Vapi adapter
checks pass. The nine-file local patch is updated and reproduced from a fresh
baseline checkout; its SHA-256 is
`099977f2a66f99987f91779d04727e523ecff6662d8a9b672cf7f82a94186033`.
Crate tests/lint, PR Gate and Rust CodeQL pass at this head. The deployed server remains the preceding goodbye
release; these diagnostics and role-specific voice prompts are local.

The read-only SMS preflight still reports `TCR_ACCEPTED` and an unassigned
sender. It also reports review placeholders in the API's saved campaign fields;
no campaign content has been changed or resubmitted. Carrier configuration,
real delivery/reply qualification and the four-party SMS rehearsal remain open.
Human phone calls remain on hold until the user explicitly returns.

Further controls contradict a read-cancellation-only diagnosis. The adapter
without a SIP/media graph fails at an audio write around 32.4 seconds, despite
inbound audio continuing; its provider call is verified ended. A raw Rust
six-worker test also stalls with uninterrupted reads, as does its timer-cancelled
variant. The experimental continuous-reader patch is therefore set aside and
has not been pushed, pinned or deployed. Two raw controls with the machine's
default 16 workers run for about a minute without write timeouts. Worker count
is a correlation to investigate, not a proven fix or a reason to weaken the
six-worker integration gate. A subsequent six-worker control with periodic TCP
statistics also completes without timeouts: the issue is intermittent or
sensitive to timing, and worker count alone does not explain it. Those successful
measurements show an open peer window, a small send buffer and no retransmissions;
they do not characterize a stalled socket. A follow-up should sample the OS
only after a write deadline fails, avoiding normal-path measurement effects.

The complete local provider/media story passes again in 49.33 seconds with the
current diagnostic patch. PR #263's crate tests/lint, PR Gate and Rust CodeQL pass at its
current head. Full live Vapi qualification remains
failed, and human calls have not resumed. A final provider readback matches all
seven transport diagnostic calls with zero active and no missing call IDs.
Authenticated human Conversation preflight still has zero active Sessions,
zero unsettled SMS and no reported configuration problems.

A subsequent six-worker raw Rust test traces I/O below TLS without sampling
TCP statistics during normal writes. It reproduces the failure after about
13.3 seconds. At the first and fiftieth consecutive write deadlines, the OS
reports 131,990 queued outbound bytes and a non-writable socket. TCP transmit
bytes stay fixed while inbound bytes and read polls continue increasing.
The peer's reported send window remains open, and retransmitted bytes remain
zero. This is evidence of actual socket backpressure; it does not yet identify
the remote service, network path or local OS component responsible. It rules
out treating a reader-only change or worker-count change as a demonstrated fix.
The additional provider call is verified ended with `silence-timed-out`.
No human phones were called, and no transport workaround has been deployed.

The next raw Rust control runs on the dedicated Linux conference host with
six workers and the same locked principal transport-library versions. It sends
3,000 frames over about a minute, finishes the synthetic booker speech and
reports zero socket-write deadlines or pending TCP writes. Fresh provider
readback confirms its call ended with
`assistant-ended-call-after-message-spoken`. Parley and Caddy remain active.
The host's instance role cannot upload diagnostic evidence to S3; the report
and log were instead retrieved through SSM. That artifact-upload failure is
separate from the successful raw audio run. This is an environment comparison,
not full Rvoip, UCTP, browser or telephone qualification. The isolated Linux
adapter-only control then completes a minute against a copy verified to match
all nine currently pinned dependency files. It queues 3,001 frames, finishes
the synthetic speech and remains live until explicit cleanup. At 55 seconds,
writer diagnostics confirm 2,757 completed messages, empty queues and zero
consecutive write timeouts. There are no write-deadline or write-failure warnings
in the saved log. Its provider call is freshly verified ended with
`assistant-ended-call-after-message-spoken`. All ten transport diagnostic calls
are confirmed ended, with no missing IDs. The deployed service is not modified.

The subsequent deployed bridge probe fails before Vapi attachment: the
synthetic SIP call's final SDP answer fails negotiation. The production endpoint
requires RTCP multiplexing; the synthetic example did not set that policy, and
its answer builder only adds `a=rtcp-mux` when the policy is enabled. The example
now agrees to multiplexing and passes `cargo check --locked --example
conference_media_peer`. The corrected Linux peer builds successfully. A retry
keeps the deployed Vapi/SIP bridge active for 65.39 seconds with continuous
received audio. Its browser stage stops at the disabled invitation button:
the transport harness had not published the UCTP browser invitation required
by the current UI. The harness now sends that scoped chat invitation before
opening the browser. After one connection attempt fails before inviting a
Session, the fresh run passes: the AI bridge remains active for 89.01 seconds,
browser takeover takes 3.90 seconds, and two-way Opus audio is detected within
238 milliseconds of WebRTC connection. The original SIP Connection and Session
remain open through takeover. Vapi is verified ended 505 milliseconds after
handoff, before the retained SIP Session ends. The peer finishes with 5,153
received frames and 235 browser-tone frames. No human phones or SMS are involved.
This qualifies the deployed synthetic transport path, not the full human story
or the candidate organizer prompt. The first retry's
provider call is verified ended. This is a test-peer and harness
change, not an Rvoip library or production behavior change. The failed probe is
confirmed cleaned up: its peer unit is inactive, its created route is absent,
and the host has zero active Sessions and unsettled SMS. No Vapi call was created
for that failed SIP probe.

The immutable `20261007-d764aa66-david-roles` source candidate is uploaded and
building separately; it has not been activated. It includes the current
role-specific booker/organizer prompts and nine-file diagnostic dependency
patch. SMS is explicitly deferred another day at the user's request. The
human worker remains stopped, and the presenter will press the page's button
before it is resumed against the saved itinerary.

## Live organizer waiting and diagnostics — October 6, 2026

Release `20261006-450850eb-goodbye` is deployed after an idle-state gate.
Public health, all five stage asset hashes and authenticated voice-only UCTP
preflight pass; the saved human Conversation has zero active Sessions and
zero unsettled SMS. This release contains expected AI shutdown handling,
booker goodbye instructions and the organizer's 180-second waiting policy.
The human has asked to hold calls until returning to the computer.

The subsequent real Vapi run fails qualification in 142.28 seconds. Its booker
goodbye and explicit UCTP hangup succeed, and the organizer receives a new call.
An invitation is accepted while David's acknowledgement is still in progress;
the AI WebSocket then fails before a browser handoff. Both provider calls are
verified ended. Vapi's authenticated structured logs show transport disconnect
and `phone-call-provider-closed-websocket`, with two prior outbound cadence
drift observations. They do not establish the underlying socket error.

The local coordinator now re-reads speech events before sending the saved
browser invitation, waits through acknowledgement across restart, and takes a
new snapshot after accepted effects before planning again. It skips a never
submitted invitation if that call has ended. Ambiguous and legacy submissions
retain their exact command IDs for reconciliation. The 52 JavaScript checks
pass; the complete local media story with the final skipped-invitation guard
and role-specific voice prompts passes in 50.48 seconds. Real Vapi qualification
of the latest local source remains open.

Local Rvoip now also has a diagnostic-only Vapi writer patch in
[PR #263](https://github.com/eisenzopf/rvoip/pull/263), commit
`053718a10a47f242ad9b7bf80ce68fa586d8d73d`. It reports media/control write
classification, sanitized socket error kind, Connection ID and deadline
expiry without logging provider error bodies or credential-bearing URLs.
All 28 library, 20 mock transport and one documentation test pass; CI is
pending. The reproducible conference patch and dependency manifest now pin
nine files. This diagnostic patch is local; the currently deployed server
still uses the preceding eight-file patch. It is not a transport fix.

The local voice prompts now distinguish the reservationist from the organizer.
David asks the booker for inventory and finishes a readback/goodbye. He asks the
organizer about pickup, gives one brief acknowledgement, announces Jonathan's
participation and waits for the actual browser handoff. The organizer no longer
receives generic reservationist instructions. Local media qualification passes;
real Vapi qualification of these latest prompts is pending before deployment.

The latest real Vapi attempt with those prompts and the nine-file diagnostic
patch fails in 52.14 seconds during the booker call, before goodbye or organizer
invitation. The writer reports sustained media write stalls: 50 consecutive
200-millisecond deadlines expire. The provider confirms its single call ended
with `phone-call-provider-closed-websocket`; no provider call remains active.
The diagnostic patch does not establish why the socket stopped accepting
writes. This result supersedes the preceding 142.28-second run as the latest
qualification attempt. No human calls were resumed while the user was away.
A fresh baseline checkout also reproduces and verifies all nine pinned patch
files through the conference dependency setup script.

## Booker goodbye and transport failure handling — October 6, 2026

David's coordinator and voice instructions explicitly respect the booker's
goodbye: one brief closing response, then UCTP `session.end` once its audio
finishes. The booker does not have to hang up. An incomplete itinerary still
ends respectfully and is reported to the owner; it cannot justify an organizer
call with invented facts. The live synthetic voice gate now includes an
attributed booker goodbye and requires an explicit UCTP hangup.

The stage ringtone's native timer invocation could throw `Illegal invocation`
and stop page polling. Its corrected timer binding is deployed and all five
served stage asset hashes match. A control disconnection also stops ringing
and disables Answer until the current invitation is reconciled. Four browser
regressions pass, including invitation loss and recovery.

A traced Vapi run revealed an unexpected AI WebSocket control-write failure
during the booker call. A previously accepted synthetic result is therefore
insufficient to qualify the full flow. The server now journals unexpected AI
transport failure separately from paired Session teardown. Deliberate UCTP
hangup and a normal remote hangup mark expected AI shutdown first; socket
write errors during that shutdown do not become false task failures. Tests
preserve failures arriving before or after Session teardown, prevent a later
normal hangup from erasing an observed failure, and ignore a retired AI route.
These are Parley integration changes; the pinned Rvoip files are unchanged.

The first updated real Vapi run proves an attributed booker goodbye, an explicit
UCTP end command and a new organizer call carrying those facts. The organizer
confirmed pickup and David announced Jonathan's participation. The browser
invitation did not arrive: a Vapi planning request failed and the provider
confirms the organizer ended with `silence-timed-out`. Both provider calls are
verified ended. This partial run does not qualify the complete story.

The organizer override now permits 180 seconds of silence while the coordinator
and browser join are pending, with a 600-second total duration limit. The booker
still has a 30-second inactivity limit and ends explicitly on goodbye. Vapi
planning requests now have a 20-second deadline, bounded retries and sanitized
timeout/network diagnostics, including failure while consuming the response
body. This addresses the measured waiting policy; it does not prove every
earlier WebSocket failure has been fixed. Updated end-to-end qualification is
pending.

The 50 JavaScript checks, 18 ordinary UCTP Rust tests and complete local
voice-only story pass (50.60 seconds). The local story includes a deliberately
lost snapshot response, organizer ringing, real SIP/WebRTC media, confirmed
phone replacement, owner approval, zero SMS and clean teardown. Real Vapi
qualification of the updated goodbye and waiting flow is still in progress. The earlier
server build was cancelled before activation and its remaining compiler
processes were stopped; the existing David release remains healthy. The human
organizer continuation, carrier SMS and venue qualification remain open.

## Human booker completion and organizer invitation — October 6, 2026

The latest human Thelve call completed David's spoken readback and goodbye.
The booker offered American Airlines 2562, departing Reno at 11 AM and arriving
in Atlanta at 4 PM, gate 2, for both travelers. These actual facts remain in
the private worker state; the synthetic RD742 itinerary must not replace them.

The worker then stopped after a UCTP snapshot response timed out. The server
journal records a timely correlated response that did not reach the worker;
no organizer call was placed. The owner subsequently tried joining the ended
booker Session, exposing premature button availability. This does not identify
a Rvoip core defect.

The worker now reconnects with bounded retries and replays an ambiguous
mutation using its saved command ID. A separate assistant-to-owner UCTP chat
message invites the owner to the active organizer Session. Only that invitation
enables **Answer organizer call**, with a visible alert and browser ringtone.
The stage assets are deployed and verified; an already open page must reload.
The 48 JavaScript tests and three browser regressions pass. The complete local
fixture passes in 47.33 seconds, including a deliberately lost snapshot reply,
ringing, browser/phone handoff and zero SMS. Three subsequent real Vapi runs
failed qualification: one reached the organizer's confirmation but issued no
browser invitation; another lost the synthetic booker's terminal detail; the
third produced no attributed organizer reply. Provider resources are confirmed
ended. The provider reports WebSocket closure and end-after-spoken-message on
these runs; they do not establish a silence-timeout cause. Invitation validation
and Session-ID diagnostics are retained for further investigation. No human
organizer call was placed during these automated checks.

The human owner page now loads the updated controls but is disconnected:
Chrome blocked credential entry while another extension panel was open. The
existing private owner token and completed booker state are preserved. Resume
only after reconnecting that page and confirming the stand-in's availability.
The full human story, new real Vapi qualification, SMS and venue gates remain open.

## David and the voice-only rehearsal — October 6, 2026

The AI assistant is named **David**. The prepared rehearsal roster, fixture
roster, planner persona and server-side voice greeting use that name. The voice
agent introduces itself as David, Jonathan's AI assistant, and asks the travel
reservationist for changes to Jonathan and Alex's canceled Reno-to-Atlanta
reservation. Requested 11 AM departure / 4 PM arrival are preferences; the
booker supplies the offered flight, times and arrival terminal.

The first human Thelve call answered and produced attributed speech in both
directions. Its AI confirmation was cut off by the external worker's end action;
the worker now waits for both speakers and re-reads speech events immediately
before submitting the saved end command. This includes speech beginning during
a planner request and across worker restart. A separate malformed singleton
recipient array now receives precise validation feedback before any call action.
No organizer call or SMS was submitted in that first partial rehearsal.

The updated **43 JavaScript checks**, ordinary Rust checks and **three browser
regressions** pass. The final complete local voice-only fixture passes in 47.02
seconds with real SIP/WebRTC media, browser-to-phone replacement, owner approval
and zero SMS submissions. The matching **real Vapi planner and voice** rehearsal
with synthetic local SIP participants passes in 119.05 seconds. Both Vapi calls
are verified ended. It verifies the organizer's attributed reply, retention of
the original organizer Connection across browser and phone participation, DTMF
confirmation, owner approval, voice-only completion and zero SMS submissions.
These automated results do not qualify the complete human/PSTN story.

An intermediate synthetic booker offered different times from those requested,
then supplied no response to David's follow-up. The provider confirmed a silence
timeout; the fixture now uses the same 11 AM / 4 PM story as the human rehearsal.
A separate organizer attachment failure exposed premature proposal generation.
The worker now requires the organizer's actual attributed answer, the browser
and confirmed owner phone handoffs on that same Session/Connection, and ended
voice before offering a voice-only proposal. The guard also checks replayed
pending proposals after restart. Failed or incomplete calls cannot supply those
facts. The earlier isolated WebSocket closure remains an unexplained historical
failure; it is not evidence of a current Rvoip defect.

The hosted David release **20261006-db39cbb0-david** and updated stage assets are
deployed. SSM reports success; public HTTPS health, served asset hashes and an
authenticated UCTP read check pass. The local external worker now forwards only
its scoped assistant identity, Vapi planning key and required runtime variables;
carrier, trunk, cloud and host administrator credentials are not inherited.
The owner subsequently started the human rehearsal; see the current result
above. The complete human story, SMS and venue gates remain open.

## Current upstream PRs and stage readiness — October 6, 2026

The subsequent owned-phone qualification passed the complete **65-second
post-join hold**. The joined phone route lasted **66.624 seconds** before the
harness ended voice; the original call lasted about **130 seconds** after answer.
The user explicitly confirmed the browser phrase and clear speech in both phone
directions after DTMF 1. The same Conversation, Session, owner Participant and
original telephone Connection were retained. Server readback reports zero active
Connections and an ended callback; the matching Vapi call retired. Private proof
is `var/conference/live/pstn-mux9k77b/live-test-observation.json`. This run's browser
return WAV is saved but has not been human-reviewed. No SMS or third-party
conference calls were placed. This supersedes the shorter post-join observation
below; the four-person full story and venue gates remain open.

All eight files in the local Rvoip conference patch are covered by two upstream
PRs opened against the audited baseline:

- [Rvoip #260](https://github.com/eisenzopf/rvoip/pull/260): late-SDP RTCP report
  lifecycle and opt-in SIP RTCP multiplexing negotiation (`fix` release label).
- [Rvoip #262](https://github.com/eisenzopf/rvoip/pull/262): authenticated opt-in
  UCTP application profile hook and WebSocket admission context (`feature`
  release label), with design issue #261, documentation and library regressions.

The PR worktrees are separate from the conference checkout. The working demo
and deployed release retain the existing pinned local patches; filing these
PRs changes neither dependency source nor runtime. The dependency verifier
still passes with the same baseline and patch fingerprint. The callback prompt
remains a Parley application change using existing Rvoip-core APIs.

The UCTP PR's 3 new application-profile tests, 51 library tests and 2 real
WebSocket conversation/admission tests pass. The RTCP patch's 395 RTP-core and
63 MediaAdapter tests passed on the identical deployed patch. Changed-file
format and diff checks pass. All 27 checks for #260 and all 20 checks for #262
passed. Both PRs remain open; passing CI does not ensure merge or inclusion in
a release.

The [presenter script and stage cues](CONFERENCE_RUNBOOK.md#presenter-script-and-stage-cues)
now contain the spoken opening, exact UI actions, sandbox facts, browser/phone
move, real-request reveal, transport explanation, closing invitation and failure
cues. The runbook also includes a concise readiness checklist. The complete
live story is not yet qualified: component media proofs do not replace it.

A fresh read-only Telnyx preflight remains blocked: `TCR_ACCEPTED`, sender
`unassigned`, no custom keyword responses and submitted workflow/HELP review
placeholders. It placed no calls or messages and changed no provider resources.
Real delivery receipts/reply attribution, three complete live rehearsals,
full live reset, venue audio/network checks,
final audible backup and frozen attendee snapshot remain gates. The preferred
Thelve route has answered the initial human booker call, but its complete
rehearsal remains unqualified; the ordinary PSTN booker is the fallback.

## Live 60-second media timeout and local Rvoip patch — October 6, 2026

The first two-owned-phone run completed actual browser-to-PSTN media, but the
remote call ended about 60 seconds after answer before phone movement committed.
A separate extended-hold run without a callback reproduced the cutoff. The
carrier BYE explicitly reports **RTP-RTCP Timeout**; the answer and ACK were on
wire. RTP headers show media in both directions and incoming carrier RTCP on a
separate port, with no outgoing RTCP. This is a media-liveness failure, not a
successful phone-move proof.

Rvoip's RTP Session created its report task only when the remote address was
known at construction, so outbound sessions created before SDP never started
reporting. The local patch starts one task and resolves the current peer at each
tick, updates the RTCP peer with later SDP, and preserves teardown. A real UDP
regression checks late SDP, retargeting and task shutdown. The standalone patch
is `patches/rvoip/rtcp-late-peer.patch`; the complete conference patch and source
fingerprints now include it. It was initially local-only; upstream PRs are
linked in the current status above.

The dedicated Telnyx trunk's RTCP mode was changed from `rtp+1` to `rtcp-mux`
to match Rvoip's current multiplexed RTP Session transport. Its existing 5-second
report frequency and disabled carrier capture setting were retained. This
conference configuration does not prove general negotiation of separate RTCP
ports. The patched release and extended live listening still require validation.

The first reporting patch was deployed (`20261006-15f693ca-phone`), and public
HTTPS/authenticated UCTP checks passed. Its owned-phone attempt reached voicemail
and was ended by the test harness; it cannot pass the extended listening gate.
Headers prove outgoing compound RTCP reports now occur, but the carrier answer
still advertised a separate RTCP port. A connection setting alone did not
negotiate multiplexing: the SIP offer lacked `a=rtcp-mux`.

The follow-up patch adds an explicit `Config::rtcp_mux_required` policy, disabled
by default and enabled on Parley's conference endpoint. Offers advertise
`rtcp-mux` and `rtcp-mux-only`, compatible answers echo multiplexing, and this
endpoint rejects SDP that declines it before committing media. The standalone
patch is `patches/rvoip/rtcp-mux-negotiation.patch`. It does not implement a
separate-port fallback; conference SIP peers must support multiplexing.
The focused SDP negotiation/refusal regression, ordinary Parley tests, and
complete local browser/SIP/phone-move scenario passed with this follow-up patch.
The follow-up release (`20261006-2576c4e1-phone`) is deployed. The carrier
answer confirms `a=rtcp-mux`, and header captures show RTCP reports in both
directions on the negotiated RTP port. An extended owned-phone run still
received **RTP-RTCP Timeout** about 60 seconds after answer: outgoing RTP stopped
after the generated browser utterance, while RTCP continued. Therefore the RTCP
patch alone does not resolve the live cutoff.

The browser test microphone was a finite AudioBufferSource with no continuing
input after speech. The harness now keeps a zero-valued ConstantSource connected
so its microphone continues producing frames during silence, like a live
microphone. This is a test-fixture correction, not proof that Rvoip survives an
absent browser media source. Extended live qualification and the missing-media
case remain outstanding. Two subsequent authorized attempts reached voicemail;
both Sessions were ended without browser takeover or callback dialing.

The next answered owned-phone run sustained outgoing RTP for 85 seconds and
the original call for about 86 seconds, ending with carrier reason **User
Triggered**, not the former timeout. The holder confirmed hearing and repeating
**orange bicycle** clearly. The callback answered but sent no outgoing RTP while
waiting for DTMF; it received a carrier **RTP-RTCP Timeout** after about 21 seconds.
The holder heard silence and did not press 1. No phone replacement committed.

Parley now has a bounded synthetic **press 1 to join** prompt on the callback's
negotiated Rvoip-core media stream. It repeats with actual G.711 silence between
utterances until confirmation, cancellation or deadline. The prompt uses the
generation-aware outbound queue, fences admitted sends and invalidates queued
frames before bridge replacement; it never takes the inbound receiver. This
application change requires no additional Rvoip patch. It supports the
conference's PCMU/PCMA mono 8 kHz codecs only. The focused codec check and full
local browser/SIP/phone scenario passed: audible callback media, continuous RTP
during a 23-second unconfirmed wait, wrong-digit refusal, cancellation, confirmed
replacement, retained original Connection, retired browser silence and teardown.
Ordinary Rust and source-kit packaging tests also passed.

Release `20261006-2c26ebaf-phone` is deployed and HTTPS/authenticated UCTP checks
passed. The live two-owned-phone run received DTMF 1 after answer and committed
the phone replacement with the same Conversation, Session, owner Participant and
original telephone Connection. Headers show actual outgoing callback RTP during
the prompt, RFC 4733 confirmation, subsequent phone audio and bidirectional
RTCP. The holder reported **“works great”** and confirmed hanging up after testing.
The carrier reports **User Triggered**, with both telephone legs ended cleanly.
This proves the live join and observed audio, but the joined route lasted only
about 10 seconds: the planned 65-second post-join hold remains unqualified.

Local validation of the reporting patch passed all 395 RTP-core unit tests, the ordinary
Parley Rust suite, and the complete conference scenario with real local
browser/SIP media and phone replacement. The live phone harness now holds the
committed phone route for 65 seconds and requires both telephone edges to remain
active before ending the Session.

## Browser-to-phone movement implemented — October 6, 2026

The accepted **Move to my phone** control uses the existing Rvoip-core prepared
outbound connection and speaking-peer replacement APIs. No additional bridge API
patch was needed for the local media test. The live RTCP patch above was
subsequently required. Parley adds scoped owner authorization, provisioned callback
routing, current speaking-route records, durable callback state and exact
request replay. Answering alone does not move media: DTMF **1** and ready
bidirectional media are required. Browser audio remains until commit.

The complete local scenario passed with actual WebRTC and SIP media: remote
telephone Connection and Session retained, owner identity unchanged, distinct
telephone tones in both directions, retired browser audio absent, wrong-digit
refusal, cancellation, duplicate request and control-socket loss. These endpoints
are local fixtures. The later live DTMF and observed two-phone audio proof is
recorded above, with the longer post-join hold still pending. The public attendee
kit currently remains the earlier verified v2 snapshot; it does not yet contain
this feature. Full conference release gates remain outstanding.

## Preferred conference booker in Thelve — October 6, 2026

The user proposed a person answering as the booker in the Thelve call-center
app. This is now the preferred conference variant, with the configured booker
PSTN phone retained as fallback. Jonathan remains the Parley WebRTC participant
who joins Jeff's existing PSTN call. A Thelve browser softphone would be a
separate WebRTC leg reached through its receiving gateway.

The receiving SIP URI or incoming number, tenant/queue routing, available seat,
transport/trust requirements and actual media are not yet qualified. Local
Thelve source provides SIP/TLS and browser-voice configuration; it is not evidence
of the live deployment. The existing Chrome CCaaS tab shows company sign-in,
and the public website could not be read by the web tool. No account, queue,
trunk or contact route was changed by this investigation. The private roster
continues to use the known PSTN routes until the Thelve route is verified.

## Live participant routes configured — October 6, 2026

The user supplied all four SMS destinations and confirmed that the booker has
PSTN only. The private draft roster now contains four distinct SMS endpoints,
with both booker and organizer voice destinations routed through the Telnyx SIP
trunk. The plan and runbook reflect this change: the live story demonstrates
PSTN phones reached over SIP/RTP, alongside WebRTC and SMS. It does not claim a
native SIP client at the booker. Participant details remain in ignored private
files. Saving the roster placed no calls or texts and does not prove reachability.
Carrier approval/assignment, final campaign workflow/HELP facts, full live runs,
room audio, audible backup and release freeze remain outstanding.

## Updated attendee download verified — October 6, 2026

The public `/kit/` download now contains the deployed SMS keyword guard,
read-only Telnyx preflight and current operator scripts. Its source archive
SHA-256 is `9b1cf814e74d5f565629895f2dacc086d3104d5b33a6b4da14bc3cf789bb907d`;
its source manifest covers 178 files. The 64 application/SDK/build/stage files
checked against the deployment archive match exactly. This supersedes the v1
public-download observations below; historical artifacts and hashes remain
available as earlier evidence.

A newly extracted copy passed dependency fingerprints, **31 JavaScript tests**,
**three packaging tests**, **61 ordinary Rust tests** and the complete local
media story. That rehearsal took 12.83 seconds: two SIP calls, nine UCTP actions,
seven planner decisions, retained organizer Connection, owner approval and four
individually addressed fixture texts. Chromium observed eight audible samples,
Opus and 109 inbound/110 outbound packets. No delivered SMS receipt was claimed.
The source was newly extracted; the previously verified project-owned Rvoip
checkout and browser/build caches were reused. This is an archive reproduction
check, not a new live-provider run.

Static publication succeeded without restarting Parley. Public assets/archive
checksum, desktop/mobile layout and the deployed closing reveal pass. The
rendered QR independently decodes to the exact kit URL. Private evidence is
`var/conference/kit-proof-v2.json`, `var/conference/live/public-kit-v2-check/`,
`var/conference/live/closing-v2-check/` and `kit-deployment-v2.json`. Publication
does not freeze the conference release; the full live gates and final snapshot
refresh remain required. The latest carrier gate still reads `TCR_ACCEPTED`,
unassigned, with draft workflow/HELP placeholders outstanding.

## SMS keyword fix deployed — October 6, 2026

The tested keyword guard is now running on the dedicated AWS host in release
`20261006-d6721330-sms`. The private build-source archive SHA-256 is
`d67213302b2afb755053fa3ab6f8546bea0bcc8fa1f8c16b51190cacb4acc03c`;
the deployed binary SHA-256 is
`71a1ff6a2bafb3697b38d01ca3d04ebc34a11a513fb1e7ddc8940b8964d055d6`.
The bundle's Rvoip files were byte-compared with the verified dedicated checkout.
The locked ARM64 release build completed successfully. The host had zero active
voice sessions and zero unsettled SMS before activation, and the updater checked
those counts again before stopping the service. It retained the previous release
and took a private SQLite recovery snapshot. Runtime settings and Caddy were not
rewritten; the current SMS sender has not been switched to the new number.

The public before/after checks match: HTTPS health/static content, authenticated
WSS and scoped reading of the existing 29-event Conversation, anonymous token
rejection, the disabled older token endpoint and unsigned-SMS rejection. The
stage HTML hash remains the verified v1 hash. The public kit manifest still
identifies the earlier tested v1 source archive; it will be refreshed at freeze.
Private evidence is `var/conference/live/sms-keyword-deployment.json`, the saved
SSM result and `sms-update-before-smoke-v2.json`/`sms-update-after-smoke.json`.
No carrier calls or SMS were placed during deployment or verification.

The latest read-only provider gate shows `TCR_ACCEPTED`, superseding the earlier
`TCR_PENDING` observations below. Carrier approval, completed number assignment
and final workflow/HELP wording are still outstanding. The live signed keyword,
delivery and human-reply gate remains unproven; local signed-fixture tests and
deployment smoke checks do not replace it.

## Provider keyword isolation — October 6, 2026

The signed SMS ingress previously treated STOP/START/HELP messages as ordinary
task replies. The pinned Telnyx SDK's typed Message omits `autoresponse_type`,
including classifications of custom keywords or natural-language opt-outs.
The webhook now checks that field in the original body after signature
verification and acknowledges classified controls separately. Reserved
whole-message keywords also stay out of task history when no classification is
present. Telnyx retains responsibility for profile-wide blocking and responses;
this guard does not send a response or fabricate proof of subscription changes.

A real HTTP/UCTP integration check signs local fixture callbacks using a test
Ed25519 key. It verifies unsigned rejection, provider-classified STOP/HELP and
YES/START, classified natural-language opt-out, reserved defaults without
classification and duplicate deliveries without task events, held inbox rows
or extra outbound messages. Unclassified YES and a sentence containing “help”
still produce exactly two deduplicated organizer replies in the canonical
Conversation. No real carrier traffic or Vapi usage is involved. All **61
ordinary Rust tests** pass; the three explicitly gated browser/live tests remain
excluded from that command. Dependency fingerprints and whitespace checks pass.
The test-only signing dependency reuses the already-locked Ed25519 package; no
package versions changed. The dedicated Rvoip patch set is unchanged.

The fix was first verified locally and has since been deployed as recorded
above. It is not in the public v1 source archive. Real keyword behavior remains
a separate gate. The complete carrier rehearsal still requires approval,
assignment and the consenting recipient/SIP roster.

## Current carrier gate and SMS preflight — October 6, 2026

The user submitted the Rudeless campaign. A fresh provider read confirms
`TCR_PENDING`; this supersedes the earlier empty-campaign observations below.
The user also authorized a new number. Its purchase completed, it is active and
supports two-way SMS on the existing demo messaging profile. Telnyx rejected its
campaign assignment with error `10036`, explicitly requiring campaign approval;
the association readback is 404. No SMS was sent, and the deployed sender was
not changed. The submitted campaign still contains the proposed-workflow draft
marker and a HELP support-contact placeholder. Those need the user's actual
opt-in implementation and support contact, rather than an invented replacement.
Private purchase, assignment and readback evidence is under
`var/conference/live/new-campaign-number-*.json`.

`scripts/preflight-telnyx-sms.mjs` now provides a read-only provider gate. It
checks active number identity, two-way SMS/profile linkage, real carrier approval,
completed campaign assignment, enabled profile and exact callback URL. It also
rejects the known review placeholders. The real new-number check correctly
reports approval, assignment and draft-content blockers. Its output contains no
phone numbers, provider IDs, keys, callback URLs or provider response bodies.
Four new regression checks cover pending/missing assignment, wrong campaign,
mock registration, wrong profile/callback, draft content and provider failures.
All **31 JavaScript regressions** pass. This is configuration evidence, not a
delivered message or signed callback round trip.

The profile currently has no custom keyword responses. Telnyx documents default
STOP/UNSUBSCRIBE blocking and START unblocking; empty custom settings do not mean
opt-out handling is absent. The campaign's branded HELP/confirmation drafts are
not deployed runtime responses. Custom response configuration and actual keyword
behavior remain unverified, along with recipient consent, carrier delivery and
the attributed human reply. The [runbook](CONFERENCE_RUNBOOK.md) includes the
provider preflight command and this distinction.

## Verified public kit snapshot and stage publication — October 6, 2026

The public v1 source archive has SHA-256
`eeb5403d8d978cf8158ed1aef0b178487473f811aba77f41368338c5d07ea34a`.
That exact extracted archive passed the dependency checks, JavaScript/package
checks and complete local SIP/browser/fixture-SMS story. Publication completed
successfully and public assets/checksums passed in
`var/conference/live/public-kit-v1-check/`. The separately published stage HTML
has SHA-256
`48901c7b59c1b6884496c06f39c67206c4965d37040b32533b450c24800b8846`;
the deployed response matches the snapshot manifest. Its closing reveal and
rendered QR were inspected, and the rendered QR independently decoded to the
exact HTTPS kit URL. Evidence is `var/conference/live/closing-v1-check/`.
This changed static HTML only; the server binary was not rebuilt or restarted.
Later operator/documentation additions are not in this already-tested snapshot;
refresh and verify the public kit at release freeze.

## Public attendee kit and closing QR — October 6, 2026

`https://conference.rudeless.ai/kit/` now serves the source kit, quickstart,
experimental profile, connector contract, reference client and verified GitHub
sharing destinations. The manifest labels a working-tree snapshot, its base
commit, individual file hashes, patched Rvoip identity and WSS control transport.
The source archive excludes runtime state, credentials, private participant
rosters, recordings and provider evidence. Three packaging tests verify private
file exclusion, known-credential rejection, symlink/unexpected-file rejection,
individual hashes and reproducible archives. A real sender number in the example
environment file was replaced by a fictional number before packaging.

The extracted candidate passed dependency verification, all 27 JavaScript
regressions, packaging checks and the complete local media story: two SIP calls,
nine UCTP actions, browser takeover, owner approval and four individually
addressed fixture texts. This proves a runnable local source download, not the
full live-provider scenario. The public HTTPS asset checks and archive checksum
pass, with desktop and phone layouts inspected. The public repository is
reachable but predates the conference code; the landing page states that the
snapshot download may be ahead of the public branch.

The QR was created only after the public landing/download were verified and was
independently decoded to the exact HTTPS kit URL using macOS Vision. The closing
reveal now includes that link and inline QR. Publication uses a separate static
directory and Caddy reload. Its initial inaccessible-directory health check
rolled back successfully; the directory was corrected to `/srv/parley-public`
and the publication succeeded. The Parley service and binary were unchanged.
Health remains 200, anonymous token provisioning 401, the older token endpoint
404 and unsigned SMS callbacks 401. Private evidence is under
`var/conference/live/public-kit-check2/`, `var/conference/kit-proof.json` and
`var/conference/verified-qr/`. Refresh the snapshot at the final release freeze.

## Resumable live preparation — October 6, 2026

The operator provisioning command now saves its exact UCTP create envelope in a
private preparation file before sending it. A retry with the same `--state` path
replays that request or finishes missing scoped credentials while preserving the
Conversation. It rejects roster/server changes, prevents concurrent preparation
for the same file and refreshes expired tokens. `--refresh-tokens` supports a
deliberate refresh before the performance. Administrator credentials are not
stored in the preparation file or printed. Preparation does not send messages,
dial calls or start the assistant worker.

All **27 JavaScript regressions** pass, including six recovery checks. The new
`e2e/conference-preparation.spec.ts` also passes against the actual local Parley
server: it deliberately loses an accepted create response, fails assistant token
issuance after saving the owner token, then resumes to the original Conversation.
Both scoped credentials authenticate and read the same roster; the journal has
one opening event and no message/voice effects. Owner-scoped closure succeeds.
This exercises real server idempotency and credential issuance with no live
provider traffic. Dependency fingerprints and whitespace checks pass; the
deployed binary and Rvoip patches are unchanged.

A fresh read still returns an empty Telnyx campaign list for the verified
Rudeless brand and 404 for the sender's campaign association. The full roster,
booker SIP route and real signed SMS round trip remain release prerequisites.
The [runbook](CONFERENCE_RUNBOOK.md) documents preparation retry, credential
refresh and using a new preparation file for the next rehearsal.

## Real PSTN/browser handoff and listening verification — October 6, 2026

The authorized telephone holder answered a real call through the AWS Parley
host, Rvoip SIP and the Telnyx trunk. Vapi heard the readiness phrase “blue
umbrella” in the telephone participant's attributed final transcript. A real
Chromium browser then joined through public HTTPS/WSS, completed ICE/DTLS and
replaced Vapi's speaking connection through UCTP. The original telephone
Connection, Session and Conversation were retained; the journal contains one
outbound dialing event for that Session. The telephone holder heard generated
browser speech say “orange bicycle,” repeated it back, and explicitly confirmed
both hearing the code and hearing their own voice in the browser's return-audio
recording. This is human-confirmed audio in both directions on an actual PSTN
call, with generated browser speech rather than a physical browser microphone.

The browser reported Opus, connected DTLS, the AWS public UDP candidate,
282 inbound and 589 outbound packets, and 25 decoded voice blocks in the
response window. Clicking Join through the Speaking state took 3,940 ms; this
is a UI handoff measurement, not network or end-to-end audio latency. Vapi's
final call record reports termination 2,033 ms after handoff, before the retained
telephone Session ended. UCTP teardown and provider termination were verified.
The private result is `verified` in `var/conference/live/pstn-mux07ako/`, alongside
the journal, browser measurements, full return WAV, a short review clip, two
actual human confirmations, and the inspected stage screenshot.

The preceding PSTN attempt reached voicemail and was ended without browser
allocation. The reusable test now detects voicemail and stops rather than
attempting a takeover without the readiness phrase. Run
`scripts/test-conference-pstn.mjs --run <private-contact.json>` only with the
telephone holder ready. `prepare-conference-speech.mjs pstn` supplies the spoken
browser prompt. The new browser speech/capture harness was first exercised
against the synthetic AWS SIP peer; that separate gate passed with its scope
explicitly labeled and saved in `var/conference/live/webrtc-mux03hn2/`.
All 21 JavaScript regressions, source fingerprint verification, syntax checks
and whitespace checks pass after these additions. The deployed server binary
and the local Rvoip patch fingerprint are unchanged.

The first completed PSTN rehearsal Conversation was also closed using the real
owner-scoped reset command on AWS. Replaying the exact decision produced one
close event, preserved all 29 journal events and left state closed. Evidence is
`var/conference/live/cloud-reset-evidence.json`. This verifies scoped live
closure/replay; it does not establish a full live SMS/outbox reset.

## SMS registration state — October 6, 2026

The user registered the Rudeless LLC brand. A fresh Telnyx read shows its
identity is `VERIFIED`, status `OK`, and it is not a mock brand. The campaign
list for that brand is empty, and the existing demo SMS sender has no number
campaign association (404). The Chrome portal is currently on the New Campaign
content-details form, with the user's customer-care description. Preserve that
in-progress form; the campaign is not yet submitted according to these reads.
The reported 48-hour approval estimate must not be treated as delivery proof.
No additional SMS was sent while checking registration. Campaign submission,
approval, sender linkage and the real signed delivery/reply round trip remain
the next carrier gates. Earlier notes about missing brand details are historical.

## Public AWS WebRTC media gate — October 6, 2026

A real Chromium instance on Jonathan's Mac connected to the deployed HTTPS/WSS
stage and exchanged WebRTC media with AWS over its public IP. The completed
gate passed: ICE and DTLS connected, the selected remote candidate was
`32.185.99.169:46000/udp`, and the browser negotiated Opus. Browser analysis
observed eight samples of the SIP endpoint's 660 Hz tone (FFT peak 656.25 Hz),
116 inbound and 122 outbound packets. The synthetic SIP endpoint independently
detected the browser's 880 Hz tone in 151 frames while still connected and 230
frames by teardown. Measured time from browser connection to audio in both
directions was approximately 200 ms; this is not a round-trip latency metric.

The browser took over the existing SIP Connection within the same Session and
Conversation. The journal recorded the original retained Connection ID and
retired Vapi Connection ID. Vapi's final call record placed provider termination
1,073 ms after the handoff, before the SIP Session ended. The browser's End voice
action ended the SIP peer; final provider state was ended. The transient peer
service and the test-created host-local Elastic IP route were removed. The
production application binary was neither changed nor restarted for this test.

Vapi REST state sometimes remained `in-progress` more than 30 seconds after the
provider's subsequently reported termination timestamp. The acceptance gate now
checks final provider timestamps after media teardown instead of treating
immediate REST status as media state. Earlier failed gate evidence is retained,
including a provider lookup timeout during a redundant teardown query. The
complete corrected gate passed in `var/conference/live/webrtc-muwzsvby/`, with
private journal, browser measurements, provider record, cleanup result and a
visually inspected handoff screenshot.

Reproduce with `e2e/run-conference-cloud-media.mjs`; deployment setup and scope
are documented in `infra/conference/README.md`. The browser and public network
path are real; microphone audio and the loopback SIP participant are synthetic.
This proves public WebRTC/SIP audio and retained-call handoff. It does not yet
prove a browser handoff into a live PSTN call, physical microphone/speaker
quality, or connectivity from the conference venue. No PSTN calls or SMS were
sent by this gate. UCTP control used WSS; QUIC was not exercised.

## AWS deployment and first PSTN connection — October 6, 2026

The user selected Vapi Internal with the `vapi-admin` profile and Rudeless LLC
as the SMS legal business. An isolated conference stack is now running in
`us-west-2`, with a dedicated VPC, static IP, private artifact bucket and encrypted
runtime parameter. The public stage is `https://conference.rudeless.ai/conference/`;
UCTP is `wss://conference.rudeless.ai/uctp`. The existing public Parley hostname
was not changed. The host has no SSH ingress, restricts SIP/RTP to Telnyx ranges,
and exposes a bounded WebRTC media range. An IAM simulation confirmed access to
the demo runtime parameter and explicit denial of an unrelated parameter.

The release build completed on Amazon Linux ARM64 after fixing bootstrap file
permissions, including Cargo's declared test files in the archive and installing
the native Opus development library. The HTTPS edge configuration and file
permissions were corrected before opening the test. The reusable infrastructure
and installer under `infra/conference/` include these fixes. The compiled binary
SHA-256 is `6cfef2c142b9fbad4810ddeb14735040d2ed9f71e982769c90364b7bd47a4441`.
The source archive SHA-256 is
`8ac1c0a9333fb7de390e2a95b0b941fac8a0ebf241d67568dfab7d98c158ac85`.
This is a deployment snapshot, not the final conference release freeze.

Public HTTPS health and static assets returned success. Anonymous conference
token provisioning returned 401; the older public widget-token endpoint returned
404. A scoped UCTP client authenticated, provisioned the stand-in Conversation,
and observed live voice, browser handoff and Telnyx capabilities. Administrator
credentials alone correctly did not grant Conversation journal membership.

One authorized call through UCTP, Rvoip SIP and the Telnyx trunk connected to the
provided PSTN destination. Vapi attached to the actual SIP Connection and
transcribed the voicemail greeting. The assistant spoke the test prompt; no
human confirmation or requested test phrase was received. The call was ended
through UCTP; the saved journal contains 28 events and nine transcript entries.
A read-only Vapi lookup independently confirmed the matching call had ended
and reported a $0.0146 Vapi charge (Telnyx voice usage is separate). This verifies
PSTN connection and incoming telephone audio, not human-confirmed two-way audio
or a live browser takeover. No second call or SMS retry was placed.

The Parley messaging profile now points at the new HTTPS SMS callback, verified
by reading it back from Telnyx. The endpoint rejects unsigned requests with 401.
Real signed receipt/reply delivery remains untested. The Rudeless LLC brand form
is prepared but not submitted: EIN, registered address and business contact
details are still needed. Carrier error 40010 remains unresolved until registration.

## Telnyx provisioning and first carrier SMS test — October 6, 2026

The user authorized Telnyx provisioning and supplied separate consenting test
destinations for voice and SMS. A dedicated credential SIP connection and active
voice number are provisioned, linked to a conversational outbound profile limited
to US/Canada, two concurrent calls, a $5 daily cap and a $0.10/minute destination
rate limit. The number order succeeded; the quoted number cost is $1 upfront and
$1/month, plus usage. Existing unrelated trunks were preserved. Credentials,
resource IDs and phone numbers are stored only in private ignored files beneath
`var/conference/`; this is configuration verification, not a completed PSTN call.

One real SMS was submitted through a scoped assistant UCTP client using the
existing Parley messaging number/profile and a fresh local database. The saved
command, correlated acknowledgement and journal establish that UCTP queued the
message and Telnyx accepted it with a provider message ID. A subsequent read-only
Telnyx message lookup showed `delivery_failed`, code `40010`: the sending number
requires 10DLC registration. The recorded charge is $0.0141. No retry was sent.
The account's brand list is empty and the sender has no campaign association;
legal business/registration information is still required from the user.

The local journal remains at `sent` (submission accepted): its public signed
callback path has not been connected to this isolated server. The independently
saved provider response is the authoritative failure evidence; do not describe
this as delivered or as proof of callback handling. Submission failures now log
only HTTP status, provider request ID and error codes, omitting bodies, contacts,
credentials and response snippets. The updated application builds successfully.

AWS sign-in is available, but the account choice remains pending because the
portal lists multiple company environments. No AWS resources were modified.
Live trunk authentication, actual PSTN audio, public callbacks, WebRTC handoff,
SMS registration/delivery and the full four-person rehearsal remain outstanding.

## Labeled visual rehearsal recording — October 6, 2026

The new `record-rehearsal` launcher records the real browser stage during the
complete local scenario, with pauses for narration at the important transitions.
It forces fixture providers and keeps the actual SIP/browser media gate. The
recorded run passed in 38.08 seconds: two SIP calls, seven planner decisions,
nine assistant UCTP actions, the retained organizer connection, owner approval
and four final fixture SMS submissions. Audio checks observed eight audible
samples, Opus and 111 inbound/112 outbound RTP packets.

The resulting WebM is 35.28 seconds, 1600×1100 at 25 fps, with no audio track.
Decoded frames were visually inspected at coordination, approval and closing.
The persistent label explicitly identifies a recorded, silent local rehearsal
with simulated people/providers. The closing frame displays the invitation to
contribute a connector or UCTP application and share it. A sidecar hashes the
video and its matching UCTP evidence JSON; both hashes and private file modes
were verified. Artifacts are under `test-results/recordings` and ignored by Git.

Playwright's project-local Chromium 1193 and FFmpeg 1011 are installed and working.
The Node 26.3 archive extractor stalled; installation completed with the bundled
Node 24 runtime, preserving the original pinned browser version. Global browser
and system FFmpeg installations were not modified.

This is a visual contingency for presenter narration, not the final audible
backup of real stand-ins, PSTN and Telnyx. The recording flag is also available
for the combined Vapi scenario; that recording mode has not yet been exercised.
The full live rehearsal, final recording, release freeze and public destination
remain outstanding.

## Saved UCTP rehearsal evidence — October 6, 2026

Added a read-only export client and operator command that drain the authorized
journal and inspect each referenced command. The resulting private JSON file
preserves canonical IDs, request/reply correlation, associated event sequence
numbers, implementation metadata, declared provider mode and explicit missing
records. It applies inspector credential/SDP redaction, omits participant routes,
and refuses to overwrite an existing file or symlink. Content remains private
until reviewed: message bodies and transcripts are intentionally retained.

The complete local scenario now exports its evidence before teardown and checks
that all nine assistant actions have actual saved replies. A fresh 11.30-second
run passed with 34 journal events and 15 inspected commands, including owner
actions and WebRTC negotiation. One journal request reference had no stored
command and is explicitly marked unavailable (404); it was not fabricated.
The retained-call audio gate still observed eight audible samples, Opus and
109 inbound/110 outbound packets. The file's permissions were verified as 0600.

All 21 JavaScript tests pass, including paginated journals with sequence gaps,
wrong-correlation/foreign-event rejection, missing evidence, credential removal,
declared-mode checks and safe file creation. This advances the inspectable trace
deliverable; it does not replace a backup recording, freeze the final build, or
prove the pending PSTN/Telnyx/conference-network gates. The ordinary Rust suite
was unchanged in this slice; its latest result remains 60 passing tests.

## Combined live Vapi planning and voice — October 6, 2026

The `live-vapi` gate passes the complete story with real Vapi Chat decisions and
real Vapi voice calls in the same fresh Conversation. Two synthetic participants
speak over local SIP: the booker supplies flight RD742, two seats, departure
16:00, arrival 17:00 and terminal C; the organizer confirms pickup and asks to
bring Jonathan into the call. The gate checks their actual attributed transcripts
and that both finish their spoken fixtures. The browser then takes over the
organizer call, exchanges real Opus/G.711 audio, and retains its SIP Connection.
The owner ends voice and approves the proposal before four final SMS submissions.

The successful run took 86.30 seconds, with seven planning decisions, nine UCTP
actions, two SIP calls, eight audible browser samples, and 111 inbound/112
outbound RTP packets. Read-only Vapi REST checks matched exactly two ended calls
to the Conversation and its Sessions. SMS remains a fixture: four final updates
were sent, zero delivered receipts were claimed. The labeled reveal screenshot
was visually inspected. This is one combined Vapi run, not a full PSTN/Telnyx
rehearsal or evidence from the conference network.

Earlier attempts exposed premature hangup during fragmented speech and an
invalid model participant ID. Vapi speech start/stop events now enter UCTP as
attributed `session.speech` facts. The worker waits while observed human speech
is active and for 1,500 ms after the latest observed human speech/transcript
activity before planning. The model must use roster participant IDs. Local
decision validation permits up to three attempts with correction feedback before
any effects are submitted; accepted or pending commands retain their existing
durable retry path. Malformed JSON still stops for diagnosis.

The JavaScript suite passes 18 tests, including speech timing across worker
restart and bounded validation correction without duplicate effects. The local
browser/media regression passes, including UCTP speech attribution, microphone
denial, handoff rollback, AI retirement and two-way audio. The full ordinary Rust
regression passes 60 tests with three explicit gates ignored. Syntax and diff
checks pass; the Rvoip patch fingerprint is unchanged. Reproduce using
`bash scripts/run-conference-demo.sh live-vapi`; see the runbook for speech
fixtures and credentials.

## Real Vapi voice and browser takeover — October 6, 2026

The opt-in `live-voice` gate now passes against the real Vapi voice service,
through the existing conference `session.invite` and Rvoip attachment path.
A synthetic organizer speaks over a local SIP/G.711 connection. Vapi recognizes
the confirmation word “pineapple,” repeats it in audible speech, and supplies
final transcripts that UCTP attributes to the organizer and assistant in the
same Conversation and Session. The browser then joins through the real stage
UI, replaces the Vapi peer, and exchanges Opus/G.711 audio with the retained SIP
participant. The call stays alive after Vapi retires; the browser's End action
ends it, with the Conversation still open.

The measured run observed **103 ms from browser media readiness to audible
two-way audio**, below the proposed 2,000 ms local target. Measurement starts at
the browser's connected event and ends when both browser and SIP endpoint have
detected the other side's known audio tone, using clocks on the same machine.
The separate click-to-Speaking UI measurement was 2,202 ms. Browser audio checks
observed eight audible samples near 656.25 Hz, 111 inbound/112 outbound RTP
packets, and Opus. Before handoff, SIP received 149 audible Vapi reply frames.
The SIP endpoint detected the browser's 880 Hz tone with a spectral-purity check.
The labeled handoff screenshot was visually inspected.

An earlier repeat exposed an unreliable test assumption: a terminal WebSocket
event was sometimes absent even though the provider call had ended. Read-only
provider checks confirmed ended conference calls. The gate now finds its exact
call by assistant, Conversation and Session metadata, then verifies that call's
REST resource has `status=ended` and an end timestamp while SIP remains alive.
The WebSocket event is recorded separately. This adds authoritative provider
evidence without treating local socket closure as proof of remote termination.

Reproduce with `bash scripts/run-conference-demo.sh live-voice`. The launcher
requires the Vapi private key, uses the selected/cached assistant, and generates
local synthetic speech with macOS `say`/`afconvert`, or accepts an explicit raw
PCM fixture on other platforms. It changes no saved provider resources and
places no PSTN calls or SMS. The separate live test binary is ignored in ordinary
Cargo runs and requires a second explicit environment opt-in; that default
ignored behavior was verified. The existing Rvoip patch fingerprint is unchanged.
The full ordinary regression run passes **60 tests with 3 explicit gates
ignored**, including the new live-voice binary. JavaScript syntax checks for the
new probe/generator and `git diff --check` pass.

This proves real Vapi voice, attributed transcripts, local SIP/browser takeover,
and provider-side termination. Real PSTN/trunk routing, Telnyx SMS delivery and
signed callbacks, the conference network, and three complete live-provider
rehearsals remain unverified. The live planning gate below and this voice
gate are separate tests; they do not yet constitute one full live-provider run.

## Live Vapi planning through the complete local scenario — October 6, 2026

Three consecutive fresh-state runs now pass with actual Vapi Chat planning and
the external worker executing communications through UCTP. Each run made seven
planning decisions and nine UCTP communications actions: booker call and end,
organizer coordination SMS and confirmation call, approval proposal, and four
final SMS submissions. The browser joined the existing organizer call, retained
its telephone Connection ID, and exchanged real Opus/G.711 audio with the local
SIP endpoint. All runs observed eight audible samples near 656.25 Hz and more
than 100 RTP packets in each direction. The proposal retained the sandbox flight,
two travelers, departure/arrival times, terminal, and pickup facts. The owner
approved before the four final updates. No delivered receipt was invented.

These are **live planning, fixture voice-provider/SMS, real local SIP/browser
media** runs. They do not satisfy the three full live-provider rehearsals or
prove actual Vapi voice, PSTN connectivity, Telnyx delivery/signatures, or the
conference network. The screenshots carry that distinction and use a separate
`conference-live-planner-` prefix. The reveal screenshot was visually inspected.

The live gate exposed failures absent from the deterministic planner fixture:
the Chat response omitted opening object characters, and `gpt-4o-mini` selected
the wrong channel, attempted an extra owner call, or waited instead of proposing
arrangements. The planner now requests a pretty-printed singleton decision
array, strictly parses it without reconstructing missing bytes, and allows
4,000 response tokens for the four updates. It defaults to the qualified
`gpt-4.1` model. The workflow prompt distinguishes SMS from web chat, browser
joining from placing a new call, and proposing from approving. The worker
supplies participant channel availability without private routing endpoints and
retains the most recent provider decision in its private state for diagnosis.
UCTP rejected the invalid owner call in a failed run; no external call resulted.

`bash scripts/run-conference-demo.sh live-planner` is the opt-in reproduction
command (Vapi usage applies). Default rehearsal and CI force fixture planning.
The live gate is bounded to 20 decisions and six minutes. **16 JavaScript tests**
pass, including malformed/ambiguous output rejection, routing privacy, and
retention of rejected decisions without submitting effects. The real UCTP
external-worker integration test also passes with lost-response/restart coverage.
The complete offline rehearsal passes after the changes, including when its
parent environment requests live planning: the launcher explicitly overrides
that flag and the fixture planner handles all seven decisions. Dependency
fingerprint verification and `git diff --check` also pass.
No Rvoip patch or provider resource modification was needed for these fixes.

## Read-only provider resource check — October 6, 2026

Authenticated GET requests to the official Vapi and Telnyx APIs returned HTTP
200 for the cached Vapi assistant, Telnyx messaging profile, sender number, and
sender messaging settings. The sender matches local configuration and is
assigned to the expected messaging profile. That profile is enabled and has an
HTTPS callback with the server's `/v1/sms/inbound` path. The assistant response
contains a model and server URL; it does not contain an explicit voice setting.
Whether the provider's effective voice configuration works remains unverified.

These checks read existing resources only. They did not start Parley, change
provider configuration, create a call, send a text, or exercise Vapi planning.
No explicit public base URL is present in the current process environment, so
the configured callback was not compared with a selected rehearsal deployment
or tested for reachability. Credentials, resource IDs, callback hosts, and phone
numbers are deliberately omitted from this record. These read-only checks alone
did not verify signed callbacks, delivery, or two-way voice. The later voice
results are recorded above; the four-person live rehearsal and public kit
publication remain outstanding.

Current state: the complete conference story now passes a clean-database local
rehearsal with an external worker, two real SIP peers, and Chrome audio. The
journal-driven stage graph and correlated request reveal are implemented.
Verified checks include **60 ordinary Rust tests, 16 JavaScript tests, 5 browser
tests, and 2 local browser/media gates**, plus the real Vapi planning and voice
gates described above. The combined Vapi gate and updated JavaScript count are
recorded in the latest section. The ordinary Rust run ignores provider/browser gates;
CI invokes the two local media gates separately, and real-provider gates remain
explicit operator commands.

The demo remains in progress: combined live Vapi/Telnyx/PSTN and conference-network
rehearsal, release freeze/recording, and a
verified public contribution kit remain outstanding. Operator launch, preflight,
and scoped close/reset are implemented and locally checked below. Earlier counts below are historical.

## Baseline — October 5, 2026

- Original Parley work and `../rvoip` were preserved.
- Local patch checkout: `../rvoip-conference`, branch `codex/uctp-conference`.
- Baseline: rvoip 0.3.12, commit `ca7861af4c9920f6949422ae1f40507bf56ede97`.
- Parley path dependencies and lockfile now resolve to that checkout.
- `cargo test --lib --tests --no-default-features --features sms-fake,uctp`: **41 passed**, zero failures. This excludes real SIP transport, WebRTC media, and Vapi voice features; those still require their own gates.
- Full conference objective remains in progress. No real phone calls or SMS messages have been sent during implementation.

The provider choices are Vapi for the assistant and Telnyx for SMS. The conference scope and completion gates remain in `CONFERENCE_DEMO_PLAN.md`.

## Conversation messaging slice — October 5, 2026

- Added an opt-in, authenticated UCTP application profile hook in the dedicated Rvoip patch checkout and WebSocket adapter.
- Added Conversation membership, explicit recipients, durable request outcomes, a Telnyx outbox, participant-attributed inbound SMS, and an audience-filtered event journal in Parley.
- `cargo test --no-default-features --features sms-fake,uctp --test uctp_conference`: **4 passed**. These use real local UCTP WebSocket connections with fake SMS delivery. They cover four recipients in one Conversation without voice, request replay across reconnect, recipient validation, cross-Conversation isolation, private history/events, ambiguous inbound reply rejection, and monotonic delivery status.
- Provider acceptance is recorded as `sent`, never invented as `delivered` or human confirmation.
- The external Vapi worker, browser voice handoff, live provider round trip, and full conference UI remain unverified and incomplete.

The closing message now explains the longer-term purpose: an open-source connector ecosystem with one shared UCTP interface for agents and human applications. Broad connector coverage remains an ambition, separate from the capabilities demonstrated by this release.

## External worker and stage walkthrough — latest local verification

The earlier counts above are historical. The current local suite,
`cargo test --lib --tests --no-default-features --features sms-fake,uctp,sip`,
passes **50 tests**, including **7 conference integration tests**. These cover
participant-scoped authorization, replay across reconnect, four-recipient SMS,
private history/events, an external JavaScript worker using a Vapi HTTP fixture,
and a real loopback SIP peer. The SIP test observes remote hangup while retaining
the Conversation for subsequent messaging. A widget identity named `api` cannot
gain administrator provisioning authority.

Legacy UCTP data now resolves through an authenticated Conversation admission
and canonical core Session binding. A regression test creates an unrelated open
Conversation first and checks the message is persisted only in the authorized
Conversation with its real participant identity.

`npm run test:conference-client` passes **5 tests**. The worker persists commands
before sending, replays the same IDs after a lost acknowledgement, validates
owner approval, and sends exactly four final updates. Its communications controls
use UCTP; Vapi supplies planning and the server-side voice adapter supplies calls.

`npx playwright test e2e/conference.spec.ts` passes the browser walkthrough:
task, proposal, owner approval, four fake SMS outcomes, and inspection of the
actual worker request and saved correlated response. The rendered stage view
was inspected. The final community reveal links the experience to two
contribution paths: connect a system or build a UCTP application.
The complete `npx playwright test` run also passes **4 browser tests**, covering
the conference view and existing widget/desk behavior with the default build.

`cargo test --lib --tests` now passes **52 tests** with default Vapi/WebRTC
features, including **8 conference tests** and the existing local WebRTC SDP/ICE
connection test. The additional receipt test proves a provider callback arriving
before its submit response survives a database restart and cannot be regressed
by a later `sent` callback. The WebRTC test does not establish the complete
browser/SIP audio handoff. The opt-in Rvoip patch is exported in `patches/rvoip/conference.patch`;
the pinned setup script recognizes it as already applied without altering the
original Rvoip checkout.

## Remaining release gates

- Finalize the submitted SMS workflow/HELP wording, obtain campaign approval and
  complete sender linkage, then verify real
  Telnyx delivery receipts and an attributed reply through the public callback.
- Run the complete eight-step story with the external worker, the booker and
  organizer's PSTN phones through Telnyx, and all four consenting SMS recipients
  in the same Conversation. The private contact routes are now configured.
  Public AWS WebRTC/SIP and human-confirmed PSTN takeover are now verified above.
- Exercise full operator preflight/reset with the four-person roster and settled
  real SMS/outbox work. Scoped live closure/replay is verified above.
- Rehearse microphone/headset/room audio and the full scenario on the conference
  network. The verified browser used generated speech from the current Mac
  network; TURN and venue connectivity have not been established.
- Record the final audible live fallback and freeze the release. Refresh the
  published attendee snapshot/QR evidence for the final build.

The complete eight-step release gate in `CONFERENCE_DEMO_PLAN.md` remains the
scope. The latest sections distinguish live provider results, synthetic SIP
harnesses and historical local checks. SMS delivery and the full four-person
story are still incomplete. The local [contribution guide](CONTRIBUTING_CONNECTORS.md)
and [runbook](CONFERENCE_RUNBOOK.md) explain how to reproduce the working slice.

## Chrome/SIP media gate — verified October 5, 2026, America/Los_Angeles

`cargo test --test uctp_conference chrome_sip_handoff -- --ignored --nocapture`
passes with real Chrome, a real loopback SIP/RTP endpoint, and a local Vapi
HTTP/WebSocket fixture. This is an explicit browser prerequisite gate; the
ordinary Cargo run lists it as ignored, and `scripts/ci.sh` runs it after browser
installation.

The gate proves:

- SIP and the Vapi adapter exchange distinct audible tones in both directions.
- A final Vapi transcript reaches an external UCTP observer under the same
  Conversation/Session and the organizer's participant ID. A partial transcript
  is not published as a final task fact.
- A browser with an unanswered offer cannot take over. After rejection, the
  original bridge still exchanges audio in both directions.
- Chrome negotiates **Opus**, while the SIP side uses **8 kHz G.711**. After
  handoff, the SIP endpoint receives the browser's 880 Hz tone and Chrome decodes
  the telephone's 660 Hz tone. The latest run measured a 656.25 Hz FFT peak,
  eight audible observations, 42 inbound RTP packets and 43 outbound packets.
- The remote SIP Connection ID and Session stay the same; no second Vapi call is
  created. Ending the Session produces remote SIP hangup.

The test exposed and drove three fixes: wait for bidirectional SIP media before
Vapi attachment; provide actual browser codec capabilities and enable the native
Opus transcoder; accept browser ICE/DTLS before permitting speaking-peer
replacement. Local SDP or a lazily allocated stream no longer grants handoff.

After these changes, **52 ordinary Rust tests**, **5 JavaScript tests**, and
**4 browser regression tests** pass, plus the explicit media gate above. This
does not substitute for live Vapi, PSTN, SMS, NAT/TURN, or conference-network
rehearsal. No external provider calls or SMS were sent by this gate.


## Deployment settings and restart recovery — October 5, 2026

Conference configuration now exposes SIP digest credentials, asserted identity,
advertised signaling/RTP addresses, bounded media ports, a WebRTC bind/static NAT
address, and separate server/browser ICE entries. Startup rejects invalid port
ranges, mismatched SIP credentials, unsupported static NAT address lists, and
missing TURN credentials. Server ICE credentials are never copied into browser
offers. Client traces and server inspection responses redact credential fields;
authenticated replay retains the original offer needed by the browser.

Startup recovery now journals interrupted SMS submissions as `unknown`, leaves
unsent queued messages eligible, and never automatically resubmits an ambiguous
provider operation. Voice Sessions become `interrupted` with stable IDs and
unknown remote termination. An owner-only UCTP `confirm_ended` attestation records
how the remote call was verified; the assistant cannot issue it. The stage view
provides this control, and the external worker pauses both planning and pending
effects until reconciliation.

Verification after these changes:

- `cargo test --lib --tests`: **55 passed**, one browser-dependent gate ignored.
- `npm run test:conference-client`: **7 passed**, including credential redaction
  and a worker restart with pending effects held until call reconciliation.
- The four existing Playwright browser regressions pass. The new focused
  recovery-panel fixture also passes: interrupted calls disable voice, an empty
  verification note sends no command, and a supplied note produces the expected
  UCTP attestation and clears the paused display. This UI fixture supplements the
  real server authorization/replay tests; it does not claim a provider call.
- `cargo build` and `cargo check --no-default-features --features sms-fake,uctp`
  both pass. No additional upstream Rvoip patch was needed for these settings
  and recovery semantics.
- Conference integration tests cover recovery across database reopening,
  recipient-scoped unknown outcomes, no duplicate recovery facts, rejection of
  assistant/empty-note/wrong-Session attestations, and exact UCTP command replay.
- The explicit Chrome/SIP media gate passes again: Opus/G.711 audio both ways,
  656.25 Hz browser FFT peak, eight audible observations, 42 inbound packets and
  44 outbound packets. The same remote SIP connection is retained.

These settings have not yet been exercised against a live trunk, TURN server,
or carrier callback endpoint. Owner attestation records verification; it does
not remotely hang up an orphaned call. See the runbook for the recovery steps.


## Private inbound routing and bounded observation — October 5, 2026

An additive holding-inbox migration preserves ambiguous SMS replies without
publishing their text to candidate Conversations. The webhook acknowledges only
after persistence. An administrator can list and resolve entries through UCTP;
ordinary owner/assistant credentials cannot do so. Resolution requires an
original candidate, a currently matching participant endpoint, and a verification
note. Message insertion, deduplication, the routing annotation, and removal from
the held list commit atomically. `scripts/conference-inbox.mjs` supplies a private
operator workflow and reuses a saved request ID when retrying resolution.

Live observers now have a maximum 30-second lease, shortened by token expiry.
The host admits at most 64, with one per physical peer/Conversation. An observer
waiting on a full output channel does not hold the command lock or erase journal
facts. Clients receive an explicit lease deadline and recover from the last
cursor they observed. Membership is refreshed before each journal batch.
History responses contain at most 500 authorized messages with cursor pagination;
the JS helper drains pages. Filtering happens before the page limit.

The full Rust suite passes **58 tests**, with the browser media gate explicitly
ignored in the ordinary run. New evidence includes a provider-shaped HTTP
callback saved once across database reopening, held-reply privacy and admin
permissions, candidate closure without implicit reassignment, CLI replay, a full
observer queue, expiry and capacity reclamation, token expiry, and a 503-message
history with participant-specific visibility. The minimal UCTP build also checks.

**9 JavaScript tests** pass. Inspector redaction now removes ICE usernames,
passwords, and key material embedded in SDP strings as well as structured TURN
credentials. Tests preserve the original negotiation object and redact only the
projection. The targeted Rust redaction test passes after this final adjustment.
No external provider calls or SMS messages were sent by these checks.


The final browser regression run passes **5 tests**, including the conference
walkthrough, recovery panel, and existing widget/desk behavior. `cargo build`
passes with the final redaction change. The exported upstream patch has the same
SHA-256 as the dedicated Rvoip checkout's binary diff; these inbox/observation
changes required no additional upstream edits.

## Complete local story and stage reveal — October 6, 2026

`cargo test --test uctp_conference -- --ignored --test-threads=1` passes both
explicit gates. The second starts with a fresh database and reads
`config/conference-scenario.json`. An external JavaScript worker uses the real
UCTP client and durable state, with local Vapi planning/voice fixtures. Through
the actual stage UI it completes:

1. Jonathan submits the travel task; the worker calls a separate SIP booker.
2. An attributed final transcript supplies an alternative; the booker call ends.
3. The organizer receives a fixture SMS; a provider-shaped inbound callback
   supplies an attributed reply after the outbound message is marked sent.
4. The worker calls a second SIP endpoint. Jonathan joins from Chrome, retaining
   the organizer's original Connection ID while replacing the AI audio bridge.
5. Jonathan ends voice and approves the sandbox proposal. Four individually
   addressed final messages become `sent`; none is invented as `delivered`.
6. Selecting the organizer's message reveals the actual external worker request,
   correlated response, and durable event under the same Conversation ID.

Both SIP calls exchange distinct tones with the Vapi media adapter. After
handoff, Chrome negotiates Opus, decodes the SIP peer's 660 Hz tone, and sends
an 880 Hz tone observed by that peer. Both remote endpoints observe teardown.
The first gate separately proves a rejected browser handoff preserves AI audio.
These are real loopback SIP/browser media checks with simulated people and
providers, not live Vapi inference, carrier/PSTN, or Telnyx delivery evidence.

The stage graph is a projection of the same journal as the timeline. Signaling,
committed audio bridges, SMS acceptance, delivery, and human replies remain
separate. Selecting an event reconstructs the graph at that event; returning to
live restores the current projection. Room view keeps the graph and controls in
one viewport with scrolling evidence panels. Source facts retain participant
attribution. Phone numbers and credentials are redacted from the projector view.
Future community connectors appear only in the explicitly labeled invitation.

The media gate saves `test-results/conference-scenario-handoff.png` and
`test-results/conference-scenario-reveal.png`, each visibly labeled as a local
rehearsal. The browser suite may clear that directory; run the explicit gates
last when retaining screenshots. The upstream Rvoip patch is unchanged by this
stage work. No external provider calls or SMS were sent.

## Operator lifecycle — October 6, 2026

Added `scripts/run-conference-demo.sh`, `scripts/preflight-conference.sh`, and
`scripts/reset-conference.mjs`. The default launcher selects the isolated local
scenario. Its explicit live-server mode requires a chosen durable database and
private admin secret, disables provider resource provisioning, and defaults
tunneling off. Preflight reads actual UCTP capabilities and state without
contacting providers; live mode requires configured SMS/voice/browser adapters
and the booker/organizer routes. It prints counts/capabilities, not tokens or
telephone numbers. Configuration availability is not provider reachability.

The owner-only `conversation.close` refuses active or interrupted Sessions and
queued, submitting, or unknown SMS submissions. A transactional close preserves
history, publishes a correlated journal event, and retires the Conversation's
reply routes. New effects fail after closure, while historical reads and exact
command replay remain available. The assistant exits on closed state with its
pending evidence intact; the stage disables communications controls. The legacy
HTTP close applies the same settled-state guard and requires an administrator
for conference Conversations. Voice preparation checks open state inside the
storage transaction before outbound activation.

The added real-WebSocket integration test covers overlapping-route preflight,
owner/assistant/cross-Conversation permissions, widget rejection through HTTP,
HTTP administrator refusal while SMS is unknown, all three unsettled outbox
states, active/interrupted voice, reconciliation, stable rejected-command replay,
successful close replay, history preservation, rejection of new sends, retirement
of late inbound routing, and reopening the database without losing closed state.
It also executes the operator close and preflight CLIs, verifying private values
do not appear in their output. A worker test proves pending commands cannot
resume after closure. The browser walkthrough now verifies closed state and
disabled stage controls after the four final messages.

The full ordinary Rust run passes **59 tests**, with two explicit media gates
ignored. **13 JavaScript tests** and **5 browser tests** pass. The final focused
close test also passes after the HTTP guard was added. See the runbook for exact
commands and the distinction between retrying an unanswered decision and issuing
a new decision after an explicit rejection.

Closing old routes cannot add a Conversation ID to SMS. Late replies to a previous
trip may still be confused with a new trip using the same local/remote number
pair; the operator must coordinate scenario changes or use separate senders.
No external provider traffic was sent by these checks.

The new `bash scripts/run-conference-demo.sh rehearsal` entry point passes the
complete scenario from fresh state: two SIP calls, nine UCTP communications
actions, four final fixture SMS submissions, and retained organizer Connection.
Chrome observed eight audible samples at a 656.25 Hz peak with 111 inbound and
111 outbound RTP packets. The separate rollback/media gate also passes after the
lifecycle changes (43 inbound/44 outbound packets). The exported Rvoip patch
still exactly matches the dedicated checkout; no upstream lifecycle patch was
required. Live-server mode was not launched and no live-provider claim is made.

## SIP failure and browser recovery gates — October 6, 2026

`cargo test --lib --tests` now passes **60 tests** with two explicit browser gates
ignored. `cargo test --test uctp_conference -- --ignored --test-threads=1` passes
both gates, including the complete scenario, after the additions below.

The new `tests/support/conference_failures.rs` drives real UCTP commands against
a loopback UDP SIP peer. It tests a busy final response, unanswered ringing until
Rvoip's actual 30-second outbound activation deadline, explicit cancellation,
and a final `200 OK` arriving after cancellation. The fixture observes final
response ACKs and cancellation on the wire; the late-answer case specifically
checks CANCEL → ACK → BYE ordering. Replaying the invitation before and after
termination preserves one SIP dialog and one Session. Each case verifies terminal
persisted state, an ended/failed core Session with all Connections detached, no
connected/assistant-attached journal event, and subsequent messaging/readiness
in the same still-open Conversation. These cases use no external carrier or AI.

The Chrome media gate now denies microphone permission in Chrome itself before
calling the real browser client. `NotAllowedError` occurs before any application
UCTP request; distinct SIP/AI tones continue in both directions. Its prior
unanswered-browser-offer rollback check remains. After successful Opus/G.711
handoff, the test lets old jitter buffers drain, then proves AI audio and provider
receive counters stop while browser-to-telephone audio continues. It retains the
same SIP Connection ID and one Vapi fixture call.

The telephone endpoint now initiates BYE at the end of that gate. The Session
ends while the Conversation stays open, and a subsequent UCTP end is harmless.
The complete-scenario gate separately proves Jonathan's UI can end voice before
approval and four final texts. The latest media result reports microphone denial,
rollback, and AI retirement verified, a 656.25 Hz browser audio peak, eight audible
observations, and 43 inbound/44 outbound RTP packets. The complete scenario passes
again with two calls and four final fixture submissions. No additional upstream
patch was needed. Live provider/network recovery remains unverified.

## Reproducible attendee dependencies and build identity — October 6, 2026

Parley now consumes its included `vendor/server-sdk-rust` snapshot directly.
The SDK manifest and Rust sources match the previous sibling dependency in
behavior, including its local Rustls ring-provider initialization; differences
in the Rust files were formatting only. The sibling checkout is no longer a
build prerequisite. `PROVENANCE.md` records the source commit, local change, MIT
license, and root-lockfile policy. CI no longer copies a sibling SDK directory.

`config/conference-dependencies.json` pins the Rvoip baseline, exported patch
SHA-256, resulting patched-file hashes, and SDK source/manifest/license hashes.
`scripts/verify-conference-dependencies.mjs` validates those sources and rejects
unrelated Rvoip changes or an unexpected SDK build script. CI and both conference
launcher modes run it; Cargo gates use `--locked`. A fresh temporary baseline
checkout plus the patch passed verification, including newly added patch files
not yet staged in Git. Deliberate patched-file edits, unexpected untracked files,
changed SDK sources, and an added SDK build script were all rejected. Temporary
verification checkouts were removed afterward.

UCTP snapshots/preflight now advertise implementation identity separately from
capabilities: host version, Rvoip baseline/revision/patch fingerprint, experimental
profile, envelope version, and WebSocket binding. The stage renders these fields
as “Rvoip 0.3.12 + conference patches” rather than stock-release support. The
contribution guide opens with the three actual client actions and contains a
fixture-first setup path using the bundled SDK. This is a local kit; publication
and its public QR destination remain pending.

Validation with the vendored SDK: **60 ordinary Rust tests** pass, plus the final
focused metadata/close integration check. **Five browser regressions** pass on
installed Chrome and again on Playwright's pinned Chromium. Both explicit media
and complete-scenario gates also pass on that Chromium, including microphone
denial, AI retirement, real Opus/G.711 audio, remote hangup, and four final fixture
texts. The updated stage screenshot was visually checked for the build banner.
The source verifier still matches the original exported Rvoip patch; no new
upstream patch was required.

The host's shared Playwright cache had missing executable/framework files.
Validation used a clean project-local cache at `var/conference-browsers`, with
`PLAYWRIGHT_BROWSERS_PATH=var/conference-browsers` and
`PARLEY_BROWSER_CHANNEL=chromium`. The installer stalled after receiving a complete
archive; its ZIP directory and all entry CRCs were verified, only that installer
process group was stopped, and the archive was extracted into the new local cache.
The shared browser cache was not replaced. This browser-install recovery is
separate from the application and provider gates. No live provider traffic was
sent during these checks.
