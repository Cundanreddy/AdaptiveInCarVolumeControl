from flask import Flask, jsonify, request, render_template_string

app = Flask(__name__)

# In-memory state controlled by the UI
STATE = {
        "cabin_db": 60.0,
        "speed_kmh": 60.0,
}

HTML = """
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>Speed / Cabin Noise UI</title>
    <style>
      * { box-sizing: border-box; }
      body {
        font-family: "Segoe UI", Arial, sans-serif;
        background: linear-gradient(135deg, #e3f2fd, #f5f5f5);
        display: flex;
        justify-content: center;
        align-items: center;
        height: 100vh;
        margin: 0;
      }
      .container {
        background: #fff;
        padding: 2rem 2.5rem;
        border-radius: 1rem;
        box-shadow: 0 6px 16px rgba(0,0,0,0.1);
        width: 360px;
        transition: all 0.3s ease;
      }
      .container:hover {
        box-shadow: 0 8px 20px rgba(0,0,0,0.15);
      }
      h2 {
        text-align: center;
        margin-bottom: 1.5rem;
        color: #1976d2;
      }
      .row {
        margin-bottom: 1.5rem;
      }
      label {
        display: flex;
        justify-content: space-between;
        margin-bottom: .5rem;
        font-weight: 500;
        color: #333;
      }
      input[type=range] {
        width: 100%;
        height: 6px;
        appearance: none;
        border-radius: 5px;
        outline: none;
        background: linear-gradient(to right, #90caf9, #1e88e5);
        transition: background 0.3s ease;
      }
      input[type=range]::-webkit-slider-thumb {
        appearance: none;
        width: 18px;
        height: 18px;
        border-radius: 50%;
        background: #fff;
        border: 2px solid #1e88e5;
        cursor: pointer;
        transition: all 0.2s ease;
      }
      input[type=range]::-webkit-slider-thumb:hover {
        transform: scale(1.1);
        background: #1e88e5;
      }
      #send {
        width: 100%;
        padding: .75rem;
        border: none;
        border-radius: .5rem;
        font-size: 1rem;
        font-weight: 600;
        background: #1976d2;
        color: #fff;
        cursor: pointer;
        transition: background 0.3s ease;
      }
      #send:hover {
        background: #125ea8;
      }
      .status {
        text-align: center;
        margin-top: 1rem;
        font-size: 0.9rem;
        color: #666;
        transition: color 0.3s ease;
      }
      .status.active {
        color: #43a047;
      }
    </style>
  </head>
  <body>
    <div class="container">
      <h2>Control Panel</h2>

      <div class="row">
        <label for="cabin">Cabin Noise (dB): <span id="cabin_val">60.0</span></label>
        <input id="cabin" type="range" min="30" max="100" step="0.1" value="60">
      </div>

      <div class="row">
        <label for="speed">Speed (km/h): <span id="speed_val">60.0</span></label>
        <input id="speed" type="range" min="0" max="200" step="0.1" value="60">
      </div>

      <button id="send">Send to Server</button>
      <div class="status" id="status">Idle</div>
    </div>

    <script>
      const cabin = document.getElementById('cabin');
      const speed = document.getElementById('speed');
      const cabinVal = document.getElementById('cabin_val');
      const speedVal = document.getElementById('speed_val');
      const send = document.getElementById('send');
      const statusEl = document.getElementById('status');

      function updateLabels() {
        cabinVal.textContent = parseFloat(cabin.value).toFixed(1);
        speedVal.textContent = parseFloat(speed.value).toFixed(1);

        // Color gradients change by values
        const cabinColor = `linear-gradient(to right, #ffb74d ${cabin.value - 30}%, #e53935)`;
        const speedColor = `linear-gradient(to right, #90caf9 ${speed.value / 2}%, #1e88e5)`;
        cabin.style.background = cabinColor;
        speed.style.background = speedColor;
      }

      cabin.addEventListener('input', () => { updateLabels(); isUserInteracting = true; scheduleSend(); });
      speed.addEventListener('input', () => { updateLabels(); isUserInteracting = true; scheduleSend(); });

      async function sendUpdate(body){
        try{
          statusEl.textContent = "Sending...";
          statusEl.classList.add('active');
          await fetch('/update', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(body) });
          statusEl.textContent = "✅ Sent";
          setTimeout(()=>{ statusEl.textContent = "Idle"; statusEl.classList.remove('active'); }, 1000);
        }catch(e){
          statusEl.textContent = "⚠️ Send failed";
          console.warn('update failed', e);
        }
      }

      let isUserInteracting = false;
      let sendTimeout = null;
      function scheduleSend(){
        if(sendTimeout) clearTimeout(sendTimeout);
        sendTimeout = setTimeout(() => {
          const body = { cabin_db: parseFloat(cabin.value), speed_kmh: parseFloat(speed.value) };
          sendUpdate(body);
          isUserInteracting = false;
        }, 200);
      }

      send.addEventListener('click', async () => {
        const body = { cabin_db: parseFloat(cabin.value), speed_kmh: parseFloat(speed.value) };
        if(sendTimeout) clearTimeout(sendTimeout);
        await sendUpdate(body);
      });

      async function poll(){
        try{
          const r = await fetch('/state');
          const j = await r.json();
          if(!isUserInteracting){
            cabin.value = j.cabin_db; speed.value = j.speed_kmh; updateLabels();
          }
        }catch(e){ console.warn('poll failed', e); }
        setTimeout(poll, 800);
      }
      updateLabels();
      poll();
    </script>
  </body>
</html>

"""

@app.route('/')
def index():
        return render_template_string(HTML)


@app.route('/state')
def state():
        return jsonify(STATE)


@app.route('/update', methods=['POST'])
def update():
        payload = request.get_json(force=True)
        if not payload:
                return jsonify({"error": "missing json"}), 400
        if 'cabin_db' in payload:
                try:
                        STATE['cabin_db'] = float(payload['cabin_db'])
                except Exception:
                        pass
        if 'speed_kmh' in payload:
                try:
                        STATE['speed_kmh'] = float(payload['speed_kmh'])
                except Exception:
                        pass
        return jsonify(STATE)


if __name__ == '__main__':
        # Bind to 127.0.0.1:5005 by default
        app.run(host='127.0.0.1', port=5005, debug=False)
