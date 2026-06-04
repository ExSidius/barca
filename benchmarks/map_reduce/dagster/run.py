"""Dagster: 1 source -> 50 mappers -> 1 reducer. 52 assets total."""

import hashlib
import json
import time
from dagster import asset, AssetIn, materialize


@asset
def source_data():
    return {"items": [f"item_{i:03d}" for i in range(50)]}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_00(data):
    item = data["items"][0]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 0, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_01(data):
    item = data["items"][1]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 1, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_02(data):
    item = data["items"][2]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 2, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_03(data):
    item = data["items"][3]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 3, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_04(data):
    item = data["items"][4]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 4, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_05(data):
    item = data["items"][5]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 5, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_06(data):
    item = data["items"][6]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 6, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_07(data):
    item = data["items"][7]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 7, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_08(data):
    item = data["items"][8]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 8, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_09(data):
    item = data["items"][9]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 9, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_10(data):
    item = data["items"][10]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 10, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_11(data):
    item = data["items"][11]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 11, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_12(data):
    item = data["items"][12]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 12, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_13(data):
    item = data["items"][13]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 13, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_14(data):
    item = data["items"][14]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 14, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_15(data):
    item = data["items"][15]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 15, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_16(data):
    item = data["items"][16]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 16, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_17(data):
    item = data["items"][17]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 17, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_18(data):
    item = data["items"][18]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 18, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_19(data):
    item = data["items"][19]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 19, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_20(data):
    item = data["items"][20]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 20, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_21(data):
    item = data["items"][21]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 21, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_22(data):
    item = data["items"][22]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 22, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_23(data):
    item = data["items"][23]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 23, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_24(data):
    item = data["items"][24]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 24, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_25(data):
    item = data["items"][25]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 25, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_26(data):
    item = data["items"][26]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 26, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_27(data):
    item = data["items"][27]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 27, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_28(data):
    item = data["items"][28]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 28, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_29(data):
    item = data["items"][29]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 29, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_30(data):
    item = data["items"][30]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 30, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_31(data):
    item = data["items"][31]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 31, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_32(data):
    item = data["items"][32]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 32, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_33(data):
    item = data["items"][33]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 33, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_34(data):
    item = data["items"][34]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 34, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_35(data):
    item = data["items"][35]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 35, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_36(data):
    item = data["items"][36]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 36, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_37(data):
    item = data["items"][37]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 37, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_38(data):
    item = data["items"][38]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 38, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_39(data):
    item = data["items"][39]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 39, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_40(data):
    item = data["items"][40]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 40, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_41(data):
    item = data["items"][41]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 41, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_42(data):
    item = data["items"][42]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 42, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_43(data):
    item = data["items"][43]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 43, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_44(data):
    item = data["items"][44]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 44, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_45(data):
    item = data["items"][45]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 45, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_46(data):
    item = data["items"][46]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 46, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_47(data):
    item = data["items"][47]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 47, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_48(data):
    item = data["items"][48]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 48, "item": item, "hash": h}


@asset(ins={"data": AssetIn(key="source_data")})
def mapper_49(data):
    item = data["items"][49]
    h = hashlib.sha256(item.encode()).hexdigest()
    return {"index": 49, "item": item, "hash": h}


@asset(
    ins={
        "m00": AssetIn(key="mapper_00"),
        "m01": AssetIn(key="mapper_01"),
        "m02": AssetIn(key="mapper_02"),
        "m03": AssetIn(key="mapper_03"),
        "m04": AssetIn(key="mapper_04"),
        "m05": AssetIn(key="mapper_05"),
        "m06": AssetIn(key="mapper_06"),
        "m07": AssetIn(key="mapper_07"),
        "m08": AssetIn(key="mapper_08"),
        "m09": AssetIn(key="mapper_09"),
        "m10": AssetIn(key="mapper_10"),
        "m11": AssetIn(key="mapper_11"),
        "m12": AssetIn(key="mapper_12"),
        "m13": AssetIn(key="mapper_13"),
        "m14": AssetIn(key="mapper_14"),
        "m15": AssetIn(key="mapper_15"),
        "m16": AssetIn(key="mapper_16"),
        "m17": AssetIn(key="mapper_17"),
        "m18": AssetIn(key="mapper_18"),
        "m19": AssetIn(key="mapper_19"),
        "m20": AssetIn(key="mapper_20"),
        "m21": AssetIn(key="mapper_21"),
        "m22": AssetIn(key="mapper_22"),
        "m23": AssetIn(key="mapper_23"),
        "m24": AssetIn(key="mapper_24"),
        "m25": AssetIn(key="mapper_25"),
        "m26": AssetIn(key="mapper_26"),
        "m27": AssetIn(key="mapper_27"),
        "m28": AssetIn(key="mapper_28"),
        "m29": AssetIn(key="mapper_29"),
        "m30": AssetIn(key="mapper_30"),
        "m31": AssetIn(key="mapper_31"),
        "m32": AssetIn(key="mapper_32"),
        "m33": AssetIn(key="mapper_33"),
        "m34": AssetIn(key="mapper_34"),
        "m35": AssetIn(key="mapper_35"),
        "m36": AssetIn(key="mapper_36"),
        "m37": AssetIn(key="mapper_37"),
        "m38": AssetIn(key="mapper_38"),
        "m39": AssetIn(key="mapper_39"),
        "m40": AssetIn(key="mapper_40"),
        "m41": AssetIn(key="mapper_41"),
        "m42": AssetIn(key="mapper_42"),
        "m43": AssetIn(key="mapper_43"),
        "m44": AssetIn(key="mapper_44"),
        "m45": AssetIn(key="mapper_45"),
        "m46": AssetIn(key="mapper_46"),
        "m47": AssetIn(key="mapper_47"),
        "m48": AssetIn(key="mapper_48"),
        "m49": AssetIn(key="mapper_49"),
    }
)
def reduce_all(
    m00,
    m01,
    m02,
    m03,
    m04,
    m05,
    m06,
    m07,
    m08,
    m09,
    m10,
    m11,
    m12,
    m13,
    m14,
    m15,
    m16,
    m17,
    m18,
    m19,
    m20,
    m21,
    m22,
    m23,
    m24,
    m25,
    m26,
    m27,
    m28,
    m29,
    m30,
    m31,
    m32,
    m33,
    m34,
    m35,
    m36,
    m37,
    m38,
    m39,
    m40,
    m41,
    m42,
    m43,
    m44,
    m45,
    m46,
    m47,
    m48,
    m49,
):
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


ALL_ASSETS = [
    source_data,
    mapper_00,
    mapper_01,
    mapper_02,
    mapper_03,
    mapper_04,
    mapper_05,
    mapper_06,
    mapper_07,
    mapper_08,
    mapper_09,
    mapper_10,
    mapper_11,
    mapper_12,
    mapper_13,
    mapper_14,
    mapper_15,
    mapper_16,
    mapper_17,
    mapper_18,
    mapper_19,
    mapper_20,
    mapper_21,
    mapper_22,
    mapper_23,
    mapper_24,
    mapper_25,
    mapper_26,
    mapper_27,
    mapper_28,
    mapper_29,
    mapper_30,
    mapper_31,
    mapper_32,
    mapper_33,
    mapper_34,
    mapper_35,
    mapper_36,
    mapper_37,
    mapper_38,
    mapper_39,
    mapper_40,
    mapper_41,
    mapper_42,
    mapper_43,
    mapper_44,
    mapper_45,
    mapper_46,
    mapper_47,
    mapper_48,
    mapper_49,
    reduce_all,
]


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = materialize(ALL_ASSETS)
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 52,
                "success": result.success,
            },
            indent=2,
        )
    )
