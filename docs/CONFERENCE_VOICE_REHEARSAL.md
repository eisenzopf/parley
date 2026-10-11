# Voice rehearsal while SMS is awaiting approval

This is a live voice variant of the same Conversation. It deliberately has no
SMS endpoints or SMS commands. David, the Vapi-powered AI assistant, calls the organizer directly
after gathering the booker's itinerary; it does not wait for a text reply.
The four-party SMS variant remains available after carrier qualification.

## Today's roles

Jonathan answers as the travel booker in the logged-in Thelve call-center seat.
Reach that seat through its supplied PSTN number and the existing Telnyx SIP
trunk. This proves a call into an existing call-center application; direct SIP
to the seat has not been qualified. A separate consenting telephone stands in
for Jeff. Jonathan uses Parley's WebRTC client and later his own callback phone.
Today's phone numbers are only in the ignored private voice-only roster. The
conference organizer's real contact is retained separately for the final demo.
No calls to Jeff or SMS messages are part of this rehearsal.

## Prepare and start

Every full rehearsal starts with a new Conversation ID and an empty worker
state. Keep one Conversation throughout that run: booker, organizer, browser,
phone and approval all belong to it. Preserve previous runs for diagnosis;
never erase or reuse their journals to simulate a clean start.

Prepare a fresh Conversation with exactly one owner, companion, booker, organizer
and assistant. Give the owner, booker and organizer their verified SIP-trunk
telephone routes. Omit every SMS endpoint. Keep the state file and scoped tokens
private; never put a token in a URL, screenshot, public kit or stage display.

```sh
node scripts/provision-conference.mjs private-voice-roster.json --state private-preparation-RUN.json
node scripts/preflight-conference.mjs path/to/provisioned.json voice-only
node scripts/start-conference-worker.mjs path/to/provisioned.json voice-only
```

Preparation uses `PARLEY_HTTP_URL`, `UCTP_URL` and `PARLEY_API_SECRET`; the worker
uses `VAPI_PRIVATE_KEY`. These commands do not create provider accounts or buy
numbers. The worker starts idle; an owner task triggers its first call. Keep the
worker process running until the rehearsal ends. Refresh expiring tokens with
`--refresh-tokens` on the same preparation file before starting. Never resume a
full-demo worker state in voice-only mode.

Replace RUN with a unique run label each time. Reusing a preparation file
recovers that same Conversation after an ambiguous provisioning result; it
does not reset the rehearsal. Before another full run, stop the old worker and
verify its calls have ended. Retire only that Conversation with
`scripts/reset-conference.mjs` and a saved close decision, then provision with
a new preparation file. Connect the page to the new ID and its new owner
token, and check that the log contains only conversation.opened before Start.
The worker must be idle with no pending effects; the presenter presses Start.

For an explicitly requested continuation, preserve the existing Conversation, worker state
and confirmed itinerary. A resumed worker can act immediately on saved work;
it is not necessarily idle. If the presenter must trigger the continuation,
leave that worker stopped, connect the owner page, and start the worker only
after the presenter sends the continuation request with the page's button.

Open `/conference/?cid=<prepared Conversation ID>&mode=voice-only`. The hosted
page defaults to its own WSS endpoint. Enter the owner token and select Connect.
It should say **Voice rehearsal · SMS deferred**. The graph updates from actual
UCTP events as the assistant places calls and connections change.

## Presenter and participants

1. Open with: “An AI shouldn't have to know which network each person uses.
   Watch David, my AI assistant, reach a call-center agent, call an organizer, bring me
   in through my browser, and move me to my phone—all in one Conversation.”
2. Select **Start coordinating**. The AI calls the Thelve booker. As the booker,
   say: “I can get them both on the 11 AM flight out of Reno, into Atlanta
   by 4 PM. Does that work?” If asked, supply the sandbox flight number and
   arrival terminal. David should acknowledge the times, gather the missing
   flight details and read the complete option back. No real change is booked.
   Let David finish his spoken confirmation, then say: “Thank you, goodbye.”
   David gives a brief goodbye and hangs up after his closing audio finishes;
   the booker does not have to hang up or wait for a timeout. If an answering AI or receptionist cannot supply the facts or connect the intended person, David asks for a transfer once, invokes `finish_call` with reason `unavailable`, and stays silent after the tool’s brief goodbye. The worker ends that Session without waiting for the other assistant to stop talking, then reports the incomplete task to the owner. It does not retry or advance without a new owner request. If the booker says
   goodbye before supplying all the facts, David ends respectfully and tells
   Jonathan what is missing instead of advancing with an invented itinerary.
3. Once the booker's Session ends, David places a **new call** to the organizer
   stand-in, carrying the actual offered itinerary in the same Conversation.
   The stand-in confirms pickup and can ask: “Should I take Jonathan and Alex
   straight to the venue, or to the hotel first?” David explains that he will
   bring Jonathan into this same call to answer. Keep that telephone connected.
   David invokes the explicit `request_browser_join` voice action. The host
   records that intent in the Conversation; the external worker sends the
   browser invitation through UCTP after speech finishes. The invitation should
   appear once. David's spoken announcement alone does not establish the bridge;
   Jonathan must answer the browser invitation before the speaking peer changes.
4. Wait for the browser to ring and display **Organizer call ready**. Jonathan
   selects **Answer organizer call** and answers the organizer's question into
   his browser microphone. The button remains disabled during the booker call.
   Browser sound requires the preceding Connect/Start gesture; the visible
   invitation remains available if the browser blocks audio.
   Both people confirm they hear each other. The graph shows WebRTC, SIP/RTP,
   the retired Vapi speaking connection and the retained organizer Connection.
5. Say: “Let me switch to my phone.” Select **Move to my phone**, answer the
   callback and press **1**. Browser audio continues until confirmation succeeds.
   Keep the organizer's original call open and verify both telephone directions.
   Show the unchanged Conversation ID, Session ID and organizer Connection ID.
   Use separate handsets for the organizer and Jonathan's callback, so answering
   the callback cannot end or put the organizer leg on hold. Keep both telephone
   directions connected for at least 70 seconds before ending the test.
6. Select **End voice**, then approve the assistant's sandbox proposal. The
   assistant records **Voice rehearsal complete · SMS deferred**. It sends no
   final texts and makes no delivery claim.
7. Select a call invitation or handoff event to reveal the actual UCTP request,
   correlated response and journal event. Explain: Rvoip implements connectors
   and media bridging; UCTP gives agents and humans a shared communications
   interface. Invite attendees to build and share connectors. Today's control
   transport is WSS; this run does not demonstrate QUIC.

## Automated qualification

```sh
CONFERENCE_DEMO_MODE=voice-only bash scripts/run-conference-demo.sh rehearsal
CONFERENCE_DEMO_MODE=voice-only bash scripts/run-conference-demo.sh live-vapi
```

The first uses fixture providers and real local SIP/WebRTC media. The second
uses real Vapi planning/voice with synthetic local SIP people. Neither places a
PSTN call. Both require zero SMS submissions, browser-to-phone peer replacement,
retained organizer Connection and clean teardown. Live Thelve seat reachability,
human audio, the organizer stand-in and the full human rehearsal require separate
confirmation; configuration preflight is not evidence of an answered call.
