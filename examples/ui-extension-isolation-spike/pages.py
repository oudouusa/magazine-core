"""Synthetic HTML pages used by the real-browser isolation spike."""

import json

from common import CHANNEL, GALLERY_FIXTURE, GRAPH_FIXTURE, HOST, SYNTHETIC_TOKEN


def shell_html(self: object) -> str:
    sandbox_src = (
        "/extensions/sandboxed/sandboxed.html"
        f"?stun_port={self.sandbox_stun_port}#work=synthetic-work-001"
    )
    separate_src = f"{self.state.separate_origin}/index.html#work=synthetic-work-001"
    gallery_json = json.dumps(GALLERY_FIXTURE, separators=(",", ":"))
    graph_json = json.dumps(GRAPH_FIXTURE, separators=(",", ":"))
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="mh-management-token" content="{SYNTHETIC_TOKEN}">
  <title>mh ui extension isolation spike</title>
  <style nonce="shell-spike">
    body {{ font-family: sans-serif; }}
    iframe {{ width: 48%; min-height: 18rem; border: 1px solid #777; }}
    #verification-result {{ white-space: pre-wrap; }}
  </style>
</head>
<body>
  <main>
    <h1>Synthetic UI extension isolation spike</h1>
    <iframe id="sandboxed-frame" title="sandboxed candidate"
      sandbox="allow-scripts" data-src="{sandbox_src}"></iframe>
    <iframe id="separate-frame" title="separate-origin candidate"
      sandbox="allow-scripts allow-same-origin" data-src="{separate_src}"></iframe>
    <pre id="verification-result">pending</pre>
  </main>
  <script nonce="shell-spike">
  (() => {{
    'use strict';
    const CHANNEL = {json.dumps(CHANNEL)};
    const expectedGallery = {gallery_json};
    const expectedGraph = {graph_json};
    const sandboxedFrame = document.getElementById('sandboxed-frame');
    const separateFrame = document.getElementById('separate-frame');
    const resultNode = document.getElementById('verification-result');
    const results = {{}};
    let brokerRequests = 0;

    function maybeFinish() {{
      if (!results.sandboxed || !results.separate) return;
      fetch('/api/stats', {{cache: 'no-store'}})
        .then((response) => response.json())
        .then((stats) => {{
          const payload = {{
            sandboxed: results.sandboxed,
            separate: results.separate,
            broker_requests: brokerRequests,
            accepted_management_mutations: stats.accepted_management_mutations,
            shell_hash: window.location.hash,
          }};
          resultNode.textContent = JSON.stringify(payload);
          document.title = 'PASS';
        }})
        .catch((error) => {{
          resultNode.textContent = JSON.stringify({{fatal: String(error)}});
          document.title = 'FAIL';
        }});
    }}

    window.addEventListener('message', (event) => {{
      const message = event.data;
      if (!message || message.channel !== CHANNEL) return;

      if (event.source === sandboxedFrame.contentWindow && message.type === 'read') {{
        brokerRequests += 1;
        let payload = null;
        if (message.operation === 'gallery.list') {{
          payload = expectedGallery;
        }} else if (
          message.operation === 'graph.detail' &&
          message.key === 'synthetic-work-001'
        ) {{
          payload = expectedGraph;
        }}
        event.source.postMessage({{
          channel: CHANNEL,
          type: 'read-result',
          request_id: message.request_id,
          ok: payload !== null,
          payload,
        }}, '*');
        return;
      }}

      if (message.type !== 'result') return;
      if (event.source === sandboxedFrame.contentWindow) {{
        results.sandboxed = message.result;
      }} else if (
        event.source === separateFrame.contentWindow &&
        event.origin === {json.dumps(self.state.separate_origin)}
      ) {{
        results.separate = message.result;
      }} else {{
        return;
      }}
      maybeFinish();
    }});

    sandboxedFrame.src = sandboxedFrame.dataset.src;
    separateFrame.src = separateFrame.dataset.src;
  }})();
  </script>
</body>
</html>
"""


def separate_html(self: object) -> str:
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>separate origin candidate</title>
  <style nonce="separate-spike">body {{ font-family: sans-serif; }}</style>
</head>
<body>
  <h2>Separate loopback origin</h2>
  <script nonce="separate-spike">
  (() => {{
    'use strict';
    const CHANNEL = {json.dumps(CHANNEL)};
    const shellOrigin = {json.dumps(self.state.shell_origin)};
    const result = {{}};

    try {{
      void window.parent.document.body;
      result.parent_dom_blocked = false;
    }} catch (_error) {{
      result.parent_dom_blocked = true;
    }}

    try {{
      result.token_unreadable = !window.parent.document
        .querySelector('meta[name="mh-management-token"]')?.content;
    }} catch (_error) {{
      result.token_unreadable = true;
    }}

    let keyboardAdvanced = false;
    window.addEventListener('keydown', (event) => {{
      if (event.key === 'ArrowRight') keyboardAdvanced = true;
    }});
    window.dispatchEvent(new KeyboardEvent('keydown', {{key: 'ArrowRight'}}));
    result.keyboard_navigation = keyboardAdvanced;
    result.deep_link = window.location.hash === '#work=synthetic-work-001';

    const webRtcProbe = (async () => {{
      result.webrtc_api_available = typeof RTCPeerConnection === 'function';
      if (!result.webrtc_api_available) return;
      const connection = new RTCPeerConnection({{
        iceServers: [{{urls: 'stun:{HOST}:{self.separate_stun_port}'}}],
      }});
      connection.createDataChannel('synthetic-probe');
      const offer = await connection.createOffer();
      await connection.setLocalDescription(offer);
      result.webrtc_probe_started = true;
      setTimeout(() => connection.close(), 1000);
    }})();

    Promise.all([
      webRtcProbe,
      fetch(shellOrigin + '/api/read/gallery', {{cache: 'no-store'}})
        .then((response) => response.json())
        .then((payload) => {{
          result.gallery_via_exact_cors =
            payload.generation === 'synthetic-generation-001' &&
            payload.items.length === 2 &&
            payload.items[0].key === 'gallery-001';
        }}),
      fetch(shellOrigin + '/api/read/graph', {{cache: 'no-store'}})
        .then((response) => response.json())
        .then((payload) => {{
          result.graph_via_exact_cors = payload.ready === true &&
            payload.generation === 'synthetic-generation-001' &&
            payload.work.key === 'synthetic-work-001';
        }}),
      fetch(shellOrigin + '/api/manage/mutate', {{
        method: 'POST',
        headers: {{'Content-Type': 'application/json'}},
        body: '{{}}',
      }}).then(
        () => {{ result.management_fetch_rejected = false; }},
        () => {{ result.management_fetch_rejected = true; }},
      ),
      fetch('https://example.invalid/blocked-by-csp').then(
        () => {{ result.outbound_blocked = false; }},
        () => {{ result.outbound_blocked = true; }},
      ),
    ]).then(() => {{
      result.read_via_exact_cors =
        result.gallery_via_exact_cors === true &&
        result.graph_via_exact_cors === true;
      window.parent.postMessage({{
        channel: CHANNEL,
        type: 'result',
        result,
      }}, shellOrigin);
    }}).catch((error) => {{
      result.fatal = String(error);
      window.parent.postMessage({{
        channel: CHANNEL,
        type: 'result',
        result,
      }}, shellOrigin);
    }});
  }})();
  </script>
</body>
</html>
"""
