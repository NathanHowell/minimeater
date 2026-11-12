# Outside-air feedforward: keep or drop (`.4.15`)

The ambient reading goes into the airflow map (`AirflowMap::rebuild`, when it has moved 2 C) and so into the airflow the controller asks for. It is not an extra bias term. The question was whether it earns its place, so the suite has a `-blind` twin of the three scenarios where the weather matters: the same cook with the TMP117 dead from the start, which makes the control task plan with the 20 C fallback (the sensor's `fault` is the only difference in events).

Twin runs (`just fw::twin::run --scenario NAME --metrics`), hold = within the scenario's hold windows:

| Scenario | Weather | hold RMS (F), sighted / blind | hold max (F) | overshoot (F) | reach (min) |
|---|---|---|---|---|---|
| hot-fast-325 | 0 C, 3 m/s | 1.12 / 1.25 | 4.15 / 3.89 | 6.10 / 6.42 | 32.2 / 32.2 |
| cold-front | 20 C to 5 C, 4 m/s at 2 h | 3.03 / 2.77 | 6.63 / 6.11 | 9.77 / 9.78 | 30.0 / 30.0 |
| hot-day | 35 C | 0.42 / 0.45 | 1.77 / 1.91 | 10.0 / 9.25 | 29.3 / 28.8 |

The two are within the noise of each other, and neither is better across the board: the feedback loop (the pit probe) takes up what the plan gets wrong, and a 15 C error in the assumed air temperature moves the draught by less than the actuator's resolution. Wind, the larger weather effect in the model, is not sensed at all.

**Decision: keep what is there, add nothing.** The map's ambient input costs nothing (a table rebuild a few times an hour, off the control path), is physically right, and keeps the plan honest when the weather is the cook's main load. A separate feedforward or bias term is dropped: it would add a tuned gain that the suite cannot show helping. The `-blind` scenarios stay as the regression check that the cook meets its bounds without the sensor; if a later change makes ambient matter (the map becoming the controller's main path, wind sensing), they are where it will show.
