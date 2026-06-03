"""Wide layers: 3 layers of 20 assets, each layer depends on previous aggregate.
60 assets total, 20-wide parallelism, periodic sync."""

import math
from barca import asset


@asset()
def layer0_00() -> dict:
    return {"layer": 0, "id": 0, "value": math.sin(0 * 0.3) * 100}


@asset()
def layer0_01() -> dict:
    return {"layer": 0, "id": 1, "value": math.sin(1 * 0.3) * 100}


@asset()
def layer0_02() -> dict:
    return {"layer": 0, "id": 2, "value": math.sin(2 * 0.3) * 100}


@asset()
def layer0_03() -> dict:
    return {"layer": 0, "id": 3, "value": math.sin(3 * 0.3) * 100}


@asset()
def layer0_04() -> dict:
    return {"layer": 0, "id": 4, "value": math.sin(4 * 0.3) * 100}


@asset()
def layer0_05() -> dict:
    return {"layer": 0, "id": 5, "value": math.sin(5 * 0.3) * 100}


@asset()
def layer0_06() -> dict:
    return {"layer": 0, "id": 6, "value": math.sin(6 * 0.3) * 100}


@asset()
def layer0_07() -> dict:
    return {"layer": 0, "id": 7, "value": math.sin(7 * 0.3) * 100}


@asset()
def layer0_08() -> dict:
    return {"layer": 0, "id": 8, "value": math.sin(8 * 0.3) * 100}


@asset()
def layer0_09() -> dict:
    return {"layer": 0, "id": 9, "value": math.sin(9 * 0.3) * 100}


@asset()
def layer0_10() -> dict:
    return {"layer": 0, "id": 10, "value": math.sin(10 * 0.3) * 100}


@asset()
def layer0_11() -> dict:
    return {"layer": 0, "id": 11, "value": math.sin(11 * 0.3) * 100}


@asset()
def layer0_12() -> dict:
    return {"layer": 0, "id": 12, "value": math.sin(12 * 0.3) * 100}


@asset()
def layer0_13() -> dict:
    return {"layer": 0, "id": 13, "value": math.sin(13 * 0.3) * 100}


@asset()
def layer0_14() -> dict:
    return {"layer": 0, "id": 14, "value": math.sin(14 * 0.3) * 100}


@asset()
def layer0_15() -> dict:
    return {"layer": 0, "id": 15, "value": math.sin(15 * 0.3) * 100}


@asset()
def layer0_16() -> dict:
    return {"layer": 0, "id": 16, "value": math.sin(16 * 0.3) * 100}


@asset()
def layer0_17() -> dict:
    return {"layer": 0, "id": 17, "value": math.sin(17 * 0.3) * 100}


@asset()
def layer0_18() -> dict:
    return {"layer": 0, "id": 18, "value": math.sin(18 * 0.3) * 100}


@asset()
def layer0_19() -> dict:
    return {"layer": 0, "id": 19, "value": math.sin(19 * 0.3) * 100}


@asset(
    inputs={
        "l0_00": layer0_00,
        "l0_01": layer0_01,
        "l0_02": layer0_02,
        "l0_03": layer0_03,
        "l0_04": layer0_04,
        "l0_05": layer0_05,
        "l0_06": layer0_06,
        "l0_07": layer0_07,
        "l0_08": layer0_08,
        "l0_09": layer0_09,
        "l0_10": layer0_10,
        "l0_11": layer0_11,
        "l0_12": layer0_12,
        "l0_13": layer0_13,
        "l0_14": layer0_14,
        "l0_15": layer0_15,
        "l0_16": layer0_16,
        "l0_17": layer0_17,
        "l0_18": layer0_18,
        "l0_19": layer0_19,
    }
)
def agg_0(
    l0_00: dict,
    l0_01: dict,
    l0_02: dict,
    l0_03: dict,
    l0_04: dict,
    l0_05: dict,
    l0_06: dict,
    l0_07: dict,
    l0_08: dict,
    l0_09: dict,
    l0_10: dict,
    l0_11: dict,
    l0_12: dict,
    l0_13: dict,
    l0_14: dict,
    l0_15: dict,
    l0_16: dict,
    l0_17: dict,
    l0_18: dict,
    l0_19: dict,
) -> dict:
    values = [
        l0_00["value"]
        + l0_01["value"]
        + l0_02["value"]
        + l0_03["value"]
        + l0_04["value"]
        + l0_05["value"]
        + l0_06["value"]
        + l0_07["value"]
        + l0_08["value"]
        + l0_09["value"]
        + l0_10["value"]
        + l0_11["value"]
        + l0_12["value"]
        + l0_13["value"]
        + l0_14["value"]
        + l0_15["value"]
        + l0_16["value"]
        + l0_17["value"]
        + l0_18["value"]
        + l0_19["value"]
    ]
    return {"sum": sum(values), "count": 20}


@asset(inputs={"agg": agg_0})
def layer1_00(agg: dict) -> dict:
    return {"layer": 1, "id": 0, "value": agg["sum"] * math.cos(0 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_01(agg: dict) -> dict:
    return {"layer": 1, "id": 1, "value": agg["sum"] * math.cos(1 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_02(agg: dict) -> dict:
    return {"layer": 1, "id": 2, "value": agg["sum"] * math.cos(2 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_03(agg: dict) -> dict:
    return {"layer": 1, "id": 3, "value": agg["sum"] * math.cos(3 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_04(agg: dict) -> dict:
    return {"layer": 1, "id": 4, "value": agg["sum"] * math.cos(4 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_05(agg: dict) -> dict:
    return {"layer": 1, "id": 5, "value": agg["sum"] * math.cos(5 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_06(agg: dict) -> dict:
    return {"layer": 1, "id": 6, "value": agg["sum"] * math.cos(6 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_07(agg: dict) -> dict:
    return {"layer": 1, "id": 7, "value": agg["sum"] * math.cos(7 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_08(agg: dict) -> dict:
    return {"layer": 1, "id": 8, "value": agg["sum"] * math.cos(8 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_09(agg: dict) -> dict:
    return {"layer": 1, "id": 9, "value": agg["sum"] * math.cos(9 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_10(agg: dict) -> dict:
    return {"layer": 1, "id": 10, "value": agg["sum"] * math.cos(10 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_11(agg: dict) -> dict:
    return {"layer": 1, "id": 11, "value": agg["sum"] * math.cos(11 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_12(agg: dict) -> dict:
    return {"layer": 1, "id": 12, "value": agg["sum"] * math.cos(12 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_13(agg: dict) -> dict:
    return {"layer": 1, "id": 13, "value": agg["sum"] * math.cos(13 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_14(agg: dict) -> dict:
    return {"layer": 1, "id": 14, "value": agg["sum"] * math.cos(14 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_15(agg: dict) -> dict:
    return {"layer": 1, "id": 15, "value": agg["sum"] * math.cos(15 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_16(agg: dict) -> dict:
    return {"layer": 1, "id": 16, "value": agg["sum"] * math.cos(16 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_17(agg: dict) -> dict:
    return {"layer": 1, "id": 17, "value": agg["sum"] * math.cos(17 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_18(agg: dict) -> dict:
    return {"layer": 1, "id": 18, "value": agg["sum"] * math.cos(18 * 0.2)}


@asset(inputs={"agg": agg_0})
def layer1_19(agg: dict) -> dict:
    return {"layer": 1, "id": 19, "value": agg["sum"] * math.cos(19 * 0.2)}


@asset(
    inputs={
        "l1_00": layer1_00,
        "l1_01": layer1_01,
        "l1_02": layer1_02,
        "l1_03": layer1_03,
        "l1_04": layer1_04,
        "l1_05": layer1_05,
        "l1_06": layer1_06,
        "l1_07": layer1_07,
        "l1_08": layer1_08,
        "l1_09": layer1_09,
        "l1_10": layer1_10,
        "l1_11": layer1_11,
        "l1_12": layer1_12,
        "l1_13": layer1_13,
        "l1_14": layer1_14,
        "l1_15": layer1_15,
        "l1_16": layer1_16,
        "l1_17": layer1_17,
        "l1_18": layer1_18,
        "l1_19": layer1_19,
    }
)
def agg_1(
    l1_00: dict,
    l1_01: dict,
    l1_02: dict,
    l1_03: dict,
    l1_04: dict,
    l1_05: dict,
    l1_06: dict,
    l1_07: dict,
    l1_08: dict,
    l1_09: dict,
    l1_10: dict,
    l1_11: dict,
    l1_12: dict,
    l1_13: dict,
    l1_14: dict,
    l1_15: dict,
    l1_16: dict,
    l1_17: dict,
    l1_18: dict,
    l1_19: dict,
) -> dict:
    values = [
        l1_00["value"]
        + l1_01["value"]
        + l1_02["value"]
        + l1_03["value"]
        + l1_04["value"]
        + l1_05["value"]
        + l1_06["value"]
        + l1_07["value"]
        + l1_08["value"]
        + l1_09["value"]
        + l1_10["value"]
        + l1_11["value"]
        + l1_12["value"]
        + l1_13["value"]
        + l1_14["value"]
        + l1_15["value"]
        + l1_16["value"]
        + l1_17["value"]
        + l1_18["value"]
        + l1_19["value"]
    ]
    return {"sum": sum(values), "count": 20}


@asset(inputs={"agg": agg_1})
def layer2_00(agg: dict) -> dict:
    return {"layer": 2, "id": 0, "value": agg["sum"] * math.tan(0 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_01(agg: dict) -> dict:
    return {"layer": 2, "id": 1, "value": agg["sum"] * math.tan(1 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_02(agg: dict) -> dict:
    return {"layer": 2, "id": 2, "value": agg["sum"] * math.tan(2 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_03(agg: dict) -> dict:
    return {"layer": 2, "id": 3, "value": agg["sum"] * math.tan(3 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_04(agg: dict) -> dict:
    return {"layer": 2, "id": 4, "value": agg["sum"] * math.tan(4 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_05(agg: dict) -> dict:
    return {"layer": 2, "id": 5, "value": agg["sum"] * math.tan(5 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_06(agg: dict) -> dict:
    return {"layer": 2, "id": 6, "value": agg["sum"] * math.tan(6 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_07(agg: dict) -> dict:
    return {"layer": 2, "id": 7, "value": agg["sum"] * math.tan(7 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_08(agg: dict) -> dict:
    return {"layer": 2, "id": 8, "value": agg["sum"] * math.tan(8 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_09(agg: dict) -> dict:
    return {"layer": 2, "id": 9, "value": agg["sum"] * math.tan(9 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_10(agg: dict) -> dict:
    return {"layer": 2, "id": 10, "value": agg["sum"] * math.tan(10 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_11(agg: dict) -> dict:
    return {"layer": 2, "id": 11, "value": agg["sum"] * math.tan(11 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_12(agg: dict) -> dict:
    return {"layer": 2, "id": 12, "value": agg["sum"] * math.tan(12 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_13(agg: dict) -> dict:
    return {"layer": 2, "id": 13, "value": agg["sum"] * math.tan(13 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_14(agg: dict) -> dict:
    return {"layer": 2, "id": 14, "value": agg["sum"] * math.tan(14 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_15(agg: dict) -> dict:
    return {"layer": 2, "id": 15, "value": agg["sum"] * math.tan(15 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_16(agg: dict) -> dict:
    return {"layer": 2, "id": 16, "value": agg["sum"] * math.tan(16 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_17(agg: dict) -> dict:
    return {"layer": 2, "id": 17, "value": agg["sum"] * math.tan(17 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_18(agg: dict) -> dict:
    return {"layer": 2, "id": 18, "value": agg["sum"] * math.tan(18 * 0.1 + 0.01)}


@asset(inputs={"agg": agg_1})
def layer2_19(agg: dict) -> dict:
    return {"layer": 2, "id": 19, "value": agg["sum"] * math.tan(19 * 0.1 + 0.01)}


@asset(
    inputs={
        "l2_00": layer2_00,
        "l2_01": layer2_01,
        "l2_02": layer2_02,
        "l2_03": layer2_03,
        "l2_04": layer2_04,
        "l2_05": layer2_05,
        "l2_06": layer2_06,
        "l2_07": layer2_07,
        "l2_08": layer2_08,
        "l2_09": layer2_09,
        "l2_10": layer2_10,
        "l2_11": layer2_11,
        "l2_12": layer2_12,
        "l2_13": layer2_13,
        "l2_14": layer2_14,
        "l2_15": layer2_15,
        "l2_16": layer2_16,
        "l2_17": layer2_17,
        "l2_18": layer2_18,
        "l2_19": layer2_19,
    }
)
def final_output(
    l2_00: dict,
    l2_01: dict,
    l2_02: dict,
    l2_03: dict,
    l2_04: dict,
    l2_05: dict,
    l2_06: dict,
    l2_07: dict,
    l2_08: dict,
    l2_09: dict,
    l2_10: dict,
    l2_11: dict,
    l2_12: dict,
    l2_13: dict,
    l2_14: dict,
    l2_15: dict,
    l2_16: dict,
    l2_17: dict,
    l2_18: dict,
    l2_19: dict,
) -> dict:
    values = [
        l2_00["value"]
        + l2_01["value"]
        + l2_02["value"]
        + l2_03["value"]
        + l2_04["value"]
        + l2_05["value"]
        + l2_06["value"]
        + l2_07["value"]
        + l2_08["value"]
        + l2_09["value"]
        + l2_10["value"]
        + l2_11["value"]
        + l2_12["value"]
        + l2_13["value"]
        + l2_14["value"]
        + l2_15["value"]
        + l2_16["value"]
        + l2_17["value"]
        + l2_18["value"]
        + l2_19["value"]
    ]
    return {"final_sum": round(sum(values), 4), "total_assets": 63}
