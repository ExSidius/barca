"""Map/Reduce: 1 source → 50 mappers → 1 reducer.
52 assets total. Tests fan-out from a single source + fan-in."""

import hashlib
from barca import asset


@asset()
def source_data() -> dict:
    return {"items": [f"item_{i:03d}" for i in range(50)]}


@asset(inputs={"data": source_data})
def mapper_00(data: dict) -> dict:
    item = data["items"][0]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 0, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_01(data: dict) -> dict:
    item = data["items"][1]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 1, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_02(data: dict) -> dict:
    item = data["items"][2]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 2, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_03(data: dict) -> dict:
    item = data["items"][3]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 3, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_04(data: dict) -> dict:
    item = data["items"][4]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 4, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_05(data: dict) -> dict:
    item = data["items"][5]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 5, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_06(data: dict) -> dict:
    item = data["items"][6]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 6, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_07(data: dict) -> dict:
    item = data["items"][7]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 7, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_08(data: dict) -> dict:
    item = data["items"][8]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 8, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_09(data: dict) -> dict:
    item = data["items"][9]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 9, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_10(data: dict) -> dict:
    item = data["items"][10]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 10, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_11(data: dict) -> dict:
    item = data["items"][11]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 11, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_12(data: dict) -> dict:
    item = data["items"][12]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 12, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_13(data: dict) -> dict:
    item = data["items"][13]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 13, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_14(data: dict) -> dict:
    item = data["items"][14]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 14, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_15(data: dict) -> dict:
    item = data["items"][15]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 15, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_16(data: dict) -> dict:
    item = data["items"][16]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 16, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_17(data: dict) -> dict:
    item = data["items"][17]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 17, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_18(data: dict) -> dict:
    item = data["items"][18]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 18, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_19(data: dict) -> dict:
    item = data["items"][19]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 19, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_20(data: dict) -> dict:
    item = data["items"][20]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 20, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_21(data: dict) -> dict:
    item = data["items"][21]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 21, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_22(data: dict) -> dict:
    item = data["items"][22]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 22, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_23(data: dict) -> dict:
    item = data["items"][23]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 23, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_24(data: dict) -> dict:
    item = data["items"][24]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 24, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_25(data: dict) -> dict:
    item = data["items"][25]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 25, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_26(data: dict) -> dict:
    item = data["items"][26]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 26, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_27(data: dict) -> dict:
    item = data["items"][27]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 27, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_28(data: dict) -> dict:
    item = data["items"][28]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 28, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_29(data: dict) -> dict:
    item = data["items"][29]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 29, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_30(data: dict) -> dict:
    item = data["items"][30]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 30, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_31(data: dict) -> dict:
    item = data["items"][31]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 31, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_32(data: dict) -> dict:
    item = data["items"][32]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 32, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_33(data: dict) -> dict:
    item = data["items"][33]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 33, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_34(data: dict) -> dict:
    item = data["items"][34]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 34, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_35(data: dict) -> dict:
    item = data["items"][35]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 35, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_36(data: dict) -> dict:
    item = data["items"][36]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 36, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_37(data: dict) -> dict:
    item = data["items"][37]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 37, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_38(data: dict) -> dict:
    item = data["items"][38]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 38, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_39(data: dict) -> dict:
    item = data["items"][39]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 39, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_40(data: dict) -> dict:
    item = data["items"][40]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 40, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_41(data: dict) -> dict:
    item = data["items"][41]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 41, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_42(data: dict) -> dict:
    item = data["items"][42]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 42, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_43(data: dict) -> dict:
    item = data["items"][43]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 43, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_44(data: dict) -> dict:
    item = data["items"][44]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 44, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_45(data: dict) -> dict:
    item = data["items"][45]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 45, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_46(data: dict) -> dict:
    item = data["items"][46]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 46, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_47(data: dict) -> dict:
    item = data["items"][47]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 47, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_48(data: dict) -> dict:
    item = data["items"][48]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 48, "item": item, "hash": h}


@asset(inputs={"data": source_data})
def mapper_49(data: dict) -> dict:
    item = data["items"][49]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 49, "item": item, "hash": h}


@asset(
    inputs={
        "m00": mapper_00,
        "m01": mapper_01,
        "m02": mapper_02,
        "m03": mapper_03,
        "m04": mapper_04,
        "m05": mapper_05,
        "m06": mapper_06,
        "m07": mapper_07,
        "m08": mapper_08,
        "m09": mapper_09,
        "m10": mapper_10,
        "m11": mapper_11,
        "m12": mapper_12,
        "m13": mapper_13,
        "m14": mapper_14,
        "m15": mapper_15,
        "m16": mapper_16,
        "m17": mapper_17,
        "m18": mapper_18,
        "m19": mapper_19,
        "m20": mapper_20,
        "m21": mapper_21,
        "m22": mapper_22,
        "m23": mapper_23,
        "m24": mapper_24,
        "m25": mapper_25,
        "m26": mapper_26,
        "m27": mapper_27,
        "m28": mapper_28,
        "m29": mapper_29,
        "m30": mapper_30,
        "m31": mapper_31,
        "m32": mapper_32,
        "m33": mapper_33,
        "m34": mapper_34,
        "m35": mapper_35,
        "m36": mapper_36,
        "m37": mapper_37,
        "m38": mapper_38,
        "m39": mapper_39,
        "m40": mapper_40,
        "m41": mapper_41,
        "m42": mapper_42,
        "m43": mapper_43,
        "m44": mapper_44,
        "m45": mapper_45,
        "m46": mapper_46,
        "m47": mapper_47,
        "m48": mapper_48,
        "m49": mapper_49,
    }
)
def reducer(
    m00: dict,
    m01: dict,
    m02: dict,
    m03: dict,
    m04: dict,
    m05: dict,
    m06: dict,
    m07: dict,
    m08: dict,
    m09: dict,
    m10: dict,
    m11: dict,
    m12: dict,
    m13: dict,
    m14: dict,
    m15: dict,
    m16: dict,
    m17: dict,
    m18: dict,
    m19: dict,
    m20: dict,
    m21: dict,
    m22: dict,
    m23: dict,
    m24: dict,
    m25: dict,
    m26: dict,
    m27: dict,
    m28: dict,
    m29: dict,
    m30: dict,
    m31: dict,
    m32: dict,
    m33: dict,
    m34: dict,
    m35: dict,
    m36: dict,
    m37: dict,
    m38: dict,
    m39: dict,
    m40: dict,
    m41: dict,
    m42: dict,
    m43: dict,
    m44: dict,
    m45: dict,
    m46: dict,
    m47: dict,
    m48: dict,
    m49: dict,
) -> dict:
    all_hashes = [
        m00["hash"]
        + m01["hash"]
        + m02["hash"]
        + m03["hash"]
        + m04["hash"]
        + m05["hash"]
        + m06["hash"]
        + m07["hash"]
        + m08["hash"]
        + m09["hash"]
        + m10["hash"]
        + m11["hash"]
        + m12["hash"]
        + m13["hash"]
        + m14["hash"]
        + m15["hash"]
        + m16["hash"]
        + m17["hash"]
        + m18["hash"]
        + m19["hash"]
        + m20["hash"]
        + m21["hash"]
        + m22["hash"]
        + m23["hash"]
        + m24["hash"]
        + m25["hash"]
        + m26["hash"]
        + m27["hash"]
        + m28["hash"]
        + m29["hash"]
        + m30["hash"]
        + m31["hash"]
        + m32["hash"]
        + m33["hash"]
        + m34["hash"]
        + m35["hash"]
        + m36["hash"]
        + m37["hash"]
        + m38["hash"]
        + m39["hash"]
        + m40["hash"]
        + m41["hash"]
        + m42["hash"]
        + m43["hash"]
        + m44["hash"]
        + m45["hash"]
        + m46["hash"]
        + m47["hash"]
        + m48["hash"]
        + m49["hash"]
    ]
    return {
        "count": 50,
        "combined_hash": hashlib.sha256("".join(all_hashes).encode()).hexdigest(),
    }
