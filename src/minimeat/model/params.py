"""Parameters for a Big Green Egg Large fed through a BBQ Guru-style intake tube.

Every value carries a confidence tag in its comment:
  [spec]  published by the manufacturer / retailer
  [forum] community measurement or report
  [est]   engineering estimate - replace with a measurement when available
"""

from dataclasses import dataclass, replace

from .units import INCH, c_to_k


@dataclass(frozen=True)
class Ambient:
    t_k: float = c_to_k(20.0)
    wind_m_s: float = 0.0


@dataclass(frozen=True)
class Egg:
    # --- shell geometry / material
    # [spec] 18.25" grid, ~162 lb total. Outer shell approximated as a prolate
    # spheroid ~0.55 m dia x 0.80 m tall (base + dome) -> ~1.25 m^2.  [est]
    outer_area_m2: float = 1.25
    inner_area_m2: float = 0.95  # [est]
    wall_m: float = 0.025  # [est] ~1" ceramic
    ceramic_k: float = 1.3  # W/m/K [est] fired stoneware/cordierite 1.0-2.0
    ceramic_cp: float = 900.0  # J/kg/K [est]
    shell_mass_kg: float = 45.0  # [est] base + dome
    firebox_mass_kg: float = 17.0  # [est] firebox + fire ring (inner node)
    emissivity_out: float = 0.9  # [est] glazed ceramic
    h_in: float = 10.0  # W/m^2/K gas->inner wall [est]
    rad_fraction: float = 0.4  # fraction of fire heat radiated straight to walls [est]
    gas_heat_capacity: float = 3000.0  # J/K: dome air + grid + grate [est]
    stack_height_m: float = 0.55  # lower vent to daisy wheel [est]

    # --- fuel / combustion
    lhv_j_kg: float = 29e6  # [est] lump charcoal 28-31 MJ/kg
    afr_stoich: float = 10.4  # kg air / kg lump (C85 H3 O7 ash3 moisture2) [est]
    excess_air: float = 1.3  # lambda [est] 1.1-2.0
    combustion_eff: float = 0.85  # CO / unburnt losses [est]
    cp_flue: float = 1100.0  # J/kg/K [est]
    fire_tau_s: float = 120.0  # heat release lag to air changes [est]
    fire_max_w: float = 20e3  # bed-limited max heat release, fully lit [est]
    lit_initial: float = 0.03  # fraction of the bed lit by a starter cube [est]
    ignition_tau_s: float = 300.0  # logistic spread time constant of the lit area [est]
    probe_tau_s: float = 20.0  # pit probe first-order lag [est]

    # --- airflow path (inside the egg)
    grate_area_m2: float = 30e-4  # [est] cast iron grate open area, partly ash-blocked
    grate_cd: float = 0.6
    bed_area_m2: float = 0.07  # [est] ~0.30 m firebox ID
    bed_depth_m: float = 0.12  # [est]
    lump_d_m: float = 0.03  # [est] mean lump size
    bed_void: float = 0.45  # [est]
    bed_t_k: float = 900.0  # [est] mean gas temp in the burning bed
    top_cd: float = 0.6
    leak_area_m2: float = 0.3e-4  # closed-damper + gasket leakage equivalent [est]


@dataclass(frozen=True)
class Intake:
    # [forum] BBQ Guru plate smooth tube 1.375" OD (Bryan Mayland, tvwbb 80033);
    # BBQ Guru-compatible stainless adapters list ~31 mm ID.
    tube_od_m: float = 1.375 * INCH
    tube_id_m: float = 0.0316
    tube_len_m: float = 0.10  # [est] exposed length + plate
    k_entry: float = 0.5  # box -> tube
    k_exit: float = 1.0  # tube -> firebox (velocity head lost)
    # Everything inside our box between fan outlet and tube: transition, open
    # flap, inlet grille. Referenced to tube velocity. Design target, set by CFD.
    k_box: float = 2.0


# Daisy wheel effective open areas, m^2 [est]; "full" from a forum airflow
# analysis giving ~3.85" max exhaust diameter.
TOP_VENT = {
    "petals cracked": 3e-4,
    "petals open": 8e-4,
    "slider 1/4": 18e-4,
    "slider 1/2": 37e-4,
    "full open": 75e-4,
}

LID_OPEN_TOP_AREA_M2 = 0.15  # [est]
LID_OPEN_G_W_K = 80.0  # extra gas->ambient conductance with the lid up [est]
LID_OPEN_EXTRA_AIR_KG_S = 1.5e-3  # air reaching the fire from above [est]


@dataclass(frozen=True)
class Scenario:
    name: str
    ambient: Ambient
    food_load_w: float = 0.0  # evaporative/heating load from food [est ~100-200 W in a stall]


SCENARIOS = [
    Scenario("mild calm 20C", Ambient(c_to_k(20.0), 0.0)),
    Scenario("mild 20C + brisket stall", Ambient(c_to_k(20.0), 0.0), food_load_w=150.0),
    Scenario("cold windy 0C 5m/s", Ambient(c_to_k(0.0), 5.0)),
    Scenario("winter -10C 8m/s", Ambient(c_to_k(-10.0), 8.0)),
]


def uncertainty_band(egg: Egg) -> dict[str, Egg]:
    """Low/nominal/high-loss variants spanning the [est] parameters that matter most."""
    return {
        "low loss": replace(egg, ceramic_k=1.0, wall_m=0.030, excess_air=1.15, combustion_eff=0.9),
        "nominal": egg,
        "high loss": replace(egg, ceramic_k=2.0, wall_m=0.020, excess_air=2.0, combustion_eff=0.75),
    }


DEFAULT_EGG = Egg()
DEFAULT_INTAKE = Intake()
