"""Unit conversions and physical constants (SI internally)."""

CFM = 4.719474e-4  # m^3/s per cubic foot per minute
MMH2O = 9.80665  # Pa per mm of water column
INCH = 0.0254  # m
LB = 0.45359237  # kg
G = 9.80665  # m/s^2
SIGMA = 5.670374e-8  # Stefan-Boltzmann, W/m^2/K^4
R_AIR = 287.05  # J/kg/K
P_ATM = 101325.0  # Pa


def f_to_k(f: float) -> float:
    return (f - 32.0) * 5.0 / 9.0 + 273.15


def k_to_f(k: float) -> float:
    return (k - 273.15) * 9.0 / 5.0 + 32.0


def c_to_k(c: float) -> float:
    return c + 273.15


def air_density(t_k: float) -> float:
    """Ideal-gas density of air (flue gas is close enough) at 1 atm."""
    return P_ATM / (R_AIR * t_k)


def air_viscosity(t_k: float) -> float:
    """Sutherland's law for air, Pa*s."""
    return 1.716e-5 * (t_k / 273.15) ** 1.5 * (273.15 + 110.4) / (t_k + 110.4)
