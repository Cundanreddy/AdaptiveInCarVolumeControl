# simple_speed_api.py
from flask import Flask, jsonify
import random
app = Flask(__name__)

@app.route('/speed')
def speed():
    # return some simulated speed value
    return jsonify({"speed": 50 + 20 * random.random()})

if __name__ == "__main__":
    app.run(port=5005)
