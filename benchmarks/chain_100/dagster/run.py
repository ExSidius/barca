"""Dagster: 100-asset linear chain."""

import json
import time
from dagster import asset, materialize, AssetIn


@asset
def asset_000():
    return {"step": 0, "value": 0}


@asset(ins={"prev": AssetIn(key="asset_000")})
def asset_001(prev):
    return {"step": 1, "value": prev["value"] + 1}


@asset(ins={"prev": AssetIn(key="asset_001")})
def asset_002(prev):
    return {"step": 2, "value": prev["value"] + 2}


@asset(ins={"prev": AssetIn(key="asset_002")})
def asset_003(prev):
    return {"step": 3, "value": prev["value"] + 3}


@asset(ins={"prev": AssetIn(key="asset_003")})
def asset_004(prev):
    return {"step": 4, "value": prev["value"] + 4}


@asset(ins={"prev": AssetIn(key="asset_004")})
def asset_005(prev):
    return {"step": 5, "value": prev["value"] + 5}


@asset(ins={"prev": AssetIn(key="asset_005")})
def asset_006(prev):
    return {"step": 6, "value": prev["value"] + 6}


@asset(ins={"prev": AssetIn(key="asset_006")})
def asset_007(prev):
    return {"step": 7, "value": prev["value"] + 7}


@asset(ins={"prev": AssetIn(key="asset_007")})
def asset_008(prev):
    return {"step": 8, "value": prev["value"] + 8}


@asset(ins={"prev": AssetIn(key="asset_008")})
def asset_009(prev):
    return {"step": 9, "value": prev["value"] + 9}


@asset(ins={"prev": AssetIn(key="asset_009")})
def asset_010(prev):
    return {"step": 10, "value": prev["value"] + 10}


@asset(ins={"prev": AssetIn(key="asset_010")})
def asset_011(prev):
    return {"step": 11, "value": prev["value"] + 11}


@asset(ins={"prev": AssetIn(key="asset_011")})
def asset_012(prev):
    return {"step": 12, "value": prev["value"] + 12}


@asset(ins={"prev": AssetIn(key="asset_012")})
def asset_013(prev):
    return {"step": 13, "value": prev["value"] + 13}


@asset(ins={"prev": AssetIn(key="asset_013")})
def asset_014(prev):
    return {"step": 14, "value": prev["value"] + 14}


@asset(ins={"prev": AssetIn(key="asset_014")})
def asset_015(prev):
    return {"step": 15, "value": prev["value"] + 15}


@asset(ins={"prev": AssetIn(key="asset_015")})
def asset_016(prev):
    return {"step": 16, "value": prev["value"] + 16}


@asset(ins={"prev": AssetIn(key="asset_016")})
def asset_017(prev):
    return {"step": 17, "value": prev["value"] + 17}


@asset(ins={"prev": AssetIn(key="asset_017")})
def asset_018(prev):
    return {"step": 18, "value": prev["value"] + 18}


@asset(ins={"prev": AssetIn(key="asset_018")})
def asset_019(prev):
    return {"step": 19, "value": prev["value"] + 19}


@asset(ins={"prev": AssetIn(key="asset_019")})
def asset_020(prev):
    return {"step": 20, "value": prev["value"] + 20}


@asset(ins={"prev": AssetIn(key="asset_020")})
def asset_021(prev):
    return {"step": 21, "value": prev["value"] + 21}


@asset(ins={"prev": AssetIn(key="asset_021")})
def asset_022(prev):
    return {"step": 22, "value": prev["value"] + 22}


@asset(ins={"prev": AssetIn(key="asset_022")})
def asset_023(prev):
    return {"step": 23, "value": prev["value"] + 23}


@asset(ins={"prev": AssetIn(key="asset_023")})
def asset_024(prev):
    return {"step": 24, "value": prev["value"] + 24}


@asset(ins={"prev": AssetIn(key="asset_024")})
def asset_025(prev):
    return {"step": 25, "value": prev["value"] + 25}


@asset(ins={"prev": AssetIn(key="asset_025")})
def asset_026(prev):
    return {"step": 26, "value": prev["value"] + 26}


@asset(ins={"prev": AssetIn(key="asset_026")})
def asset_027(prev):
    return {"step": 27, "value": prev["value"] + 27}


@asset(ins={"prev": AssetIn(key="asset_027")})
def asset_028(prev):
    return {"step": 28, "value": prev["value"] + 28}


@asset(ins={"prev": AssetIn(key="asset_028")})
def asset_029(prev):
    return {"step": 29, "value": prev["value"] + 29}


@asset(ins={"prev": AssetIn(key="asset_029")})
def asset_030(prev):
    return {"step": 30, "value": prev["value"] + 30}


@asset(ins={"prev": AssetIn(key="asset_030")})
def asset_031(prev):
    return {"step": 31, "value": prev["value"] + 31}


@asset(ins={"prev": AssetIn(key="asset_031")})
def asset_032(prev):
    return {"step": 32, "value": prev["value"] + 32}


@asset(ins={"prev": AssetIn(key="asset_032")})
def asset_033(prev):
    return {"step": 33, "value": prev["value"] + 33}


@asset(ins={"prev": AssetIn(key="asset_033")})
def asset_034(prev):
    return {"step": 34, "value": prev["value"] + 34}


@asset(ins={"prev": AssetIn(key="asset_034")})
def asset_035(prev):
    return {"step": 35, "value": prev["value"] + 35}


@asset(ins={"prev": AssetIn(key="asset_035")})
def asset_036(prev):
    return {"step": 36, "value": prev["value"] + 36}


@asset(ins={"prev": AssetIn(key="asset_036")})
def asset_037(prev):
    return {"step": 37, "value": prev["value"] + 37}


@asset(ins={"prev": AssetIn(key="asset_037")})
def asset_038(prev):
    return {"step": 38, "value": prev["value"] + 38}


@asset(ins={"prev": AssetIn(key="asset_038")})
def asset_039(prev):
    return {"step": 39, "value": prev["value"] + 39}


@asset(ins={"prev": AssetIn(key="asset_039")})
def asset_040(prev):
    return {"step": 40, "value": prev["value"] + 40}


@asset(ins={"prev": AssetIn(key="asset_040")})
def asset_041(prev):
    return {"step": 41, "value": prev["value"] + 41}


@asset(ins={"prev": AssetIn(key="asset_041")})
def asset_042(prev):
    return {"step": 42, "value": prev["value"] + 42}


@asset(ins={"prev": AssetIn(key="asset_042")})
def asset_043(prev):
    return {"step": 43, "value": prev["value"] + 43}


@asset(ins={"prev": AssetIn(key="asset_043")})
def asset_044(prev):
    return {"step": 44, "value": prev["value"] + 44}


@asset(ins={"prev": AssetIn(key="asset_044")})
def asset_045(prev):
    return {"step": 45, "value": prev["value"] + 45}


@asset(ins={"prev": AssetIn(key="asset_045")})
def asset_046(prev):
    return {"step": 46, "value": prev["value"] + 46}


@asset(ins={"prev": AssetIn(key="asset_046")})
def asset_047(prev):
    return {"step": 47, "value": prev["value"] + 47}


@asset(ins={"prev": AssetIn(key="asset_047")})
def asset_048(prev):
    return {"step": 48, "value": prev["value"] + 48}


@asset(ins={"prev": AssetIn(key="asset_048")})
def asset_049(prev):
    return {"step": 49, "value": prev["value"] + 49}


@asset(ins={"prev": AssetIn(key="asset_049")})
def asset_050(prev):
    return {"step": 50, "value": prev["value"] + 50}


@asset(ins={"prev": AssetIn(key="asset_050")})
def asset_051(prev):
    return {"step": 51, "value": prev["value"] + 51}


@asset(ins={"prev": AssetIn(key="asset_051")})
def asset_052(prev):
    return {"step": 52, "value": prev["value"] + 52}


@asset(ins={"prev": AssetIn(key="asset_052")})
def asset_053(prev):
    return {"step": 53, "value": prev["value"] + 53}


@asset(ins={"prev": AssetIn(key="asset_053")})
def asset_054(prev):
    return {"step": 54, "value": prev["value"] + 54}


@asset(ins={"prev": AssetIn(key="asset_054")})
def asset_055(prev):
    return {"step": 55, "value": prev["value"] + 55}


@asset(ins={"prev": AssetIn(key="asset_055")})
def asset_056(prev):
    return {"step": 56, "value": prev["value"] + 56}


@asset(ins={"prev": AssetIn(key="asset_056")})
def asset_057(prev):
    return {"step": 57, "value": prev["value"] + 57}


@asset(ins={"prev": AssetIn(key="asset_057")})
def asset_058(prev):
    return {"step": 58, "value": prev["value"] + 58}


@asset(ins={"prev": AssetIn(key="asset_058")})
def asset_059(prev):
    return {"step": 59, "value": prev["value"] + 59}


@asset(ins={"prev": AssetIn(key="asset_059")})
def asset_060(prev):
    return {"step": 60, "value": prev["value"] + 60}


@asset(ins={"prev": AssetIn(key="asset_060")})
def asset_061(prev):
    return {"step": 61, "value": prev["value"] + 61}


@asset(ins={"prev": AssetIn(key="asset_061")})
def asset_062(prev):
    return {"step": 62, "value": prev["value"] + 62}


@asset(ins={"prev": AssetIn(key="asset_062")})
def asset_063(prev):
    return {"step": 63, "value": prev["value"] + 63}


@asset(ins={"prev": AssetIn(key="asset_063")})
def asset_064(prev):
    return {"step": 64, "value": prev["value"] + 64}


@asset(ins={"prev": AssetIn(key="asset_064")})
def asset_065(prev):
    return {"step": 65, "value": prev["value"] + 65}


@asset(ins={"prev": AssetIn(key="asset_065")})
def asset_066(prev):
    return {"step": 66, "value": prev["value"] + 66}


@asset(ins={"prev": AssetIn(key="asset_066")})
def asset_067(prev):
    return {"step": 67, "value": prev["value"] + 67}


@asset(ins={"prev": AssetIn(key="asset_067")})
def asset_068(prev):
    return {"step": 68, "value": prev["value"] + 68}


@asset(ins={"prev": AssetIn(key="asset_068")})
def asset_069(prev):
    return {"step": 69, "value": prev["value"] + 69}


@asset(ins={"prev": AssetIn(key="asset_069")})
def asset_070(prev):
    return {"step": 70, "value": prev["value"] + 70}


@asset(ins={"prev": AssetIn(key="asset_070")})
def asset_071(prev):
    return {"step": 71, "value": prev["value"] + 71}


@asset(ins={"prev": AssetIn(key="asset_071")})
def asset_072(prev):
    return {"step": 72, "value": prev["value"] + 72}


@asset(ins={"prev": AssetIn(key="asset_072")})
def asset_073(prev):
    return {"step": 73, "value": prev["value"] + 73}


@asset(ins={"prev": AssetIn(key="asset_073")})
def asset_074(prev):
    return {"step": 74, "value": prev["value"] + 74}


@asset(ins={"prev": AssetIn(key="asset_074")})
def asset_075(prev):
    return {"step": 75, "value": prev["value"] + 75}


@asset(ins={"prev": AssetIn(key="asset_075")})
def asset_076(prev):
    return {"step": 76, "value": prev["value"] + 76}


@asset(ins={"prev": AssetIn(key="asset_076")})
def asset_077(prev):
    return {"step": 77, "value": prev["value"] + 77}


@asset(ins={"prev": AssetIn(key="asset_077")})
def asset_078(prev):
    return {"step": 78, "value": prev["value"] + 78}


@asset(ins={"prev": AssetIn(key="asset_078")})
def asset_079(prev):
    return {"step": 79, "value": prev["value"] + 79}


@asset(ins={"prev": AssetIn(key="asset_079")})
def asset_080(prev):
    return {"step": 80, "value": prev["value"] + 80}


@asset(ins={"prev": AssetIn(key="asset_080")})
def asset_081(prev):
    return {"step": 81, "value": prev["value"] + 81}


@asset(ins={"prev": AssetIn(key="asset_081")})
def asset_082(prev):
    return {"step": 82, "value": prev["value"] + 82}


@asset(ins={"prev": AssetIn(key="asset_082")})
def asset_083(prev):
    return {"step": 83, "value": prev["value"] + 83}


@asset(ins={"prev": AssetIn(key="asset_083")})
def asset_084(prev):
    return {"step": 84, "value": prev["value"] + 84}


@asset(ins={"prev": AssetIn(key="asset_084")})
def asset_085(prev):
    return {"step": 85, "value": prev["value"] + 85}


@asset(ins={"prev": AssetIn(key="asset_085")})
def asset_086(prev):
    return {"step": 86, "value": prev["value"] + 86}


@asset(ins={"prev": AssetIn(key="asset_086")})
def asset_087(prev):
    return {"step": 87, "value": prev["value"] + 87}


@asset(ins={"prev": AssetIn(key="asset_087")})
def asset_088(prev):
    return {"step": 88, "value": prev["value"] + 88}


@asset(ins={"prev": AssetIn(key="asset_088")})
def asset_089(prev):
    return {"step": 89, "value": prev["value"] + 89}


@asset(ins={"prev": AssetIn(key="asset_089")})
def asset_090(prev):
    return {"step": 90, "value": prev["value"] + 90}


@asset(ins={"prev": AssetIn(key="asset_090")})
def asset_091(prev):
    return {"step": 91, "value": prev["value"] + 91}


@asset(ins={"prev": AssetIn(key="asset_091")})
def asset_092(prev):
    return {"step": 92, "value": prev["value"] + 92}


@asset(ins={"prev": AssetIn(key="asset_092")})
def asset_093(prev):
    return {"step": 93, "value": prev["value"] + 93}


@asset(ins={"prev": AssetIn(key="asset_093")})
def asset_094(prev):
    return {"step": 94, "value": prev["value"] + 94}


@asset(ins={"prev": AssetIn(key="asset_094")})
def asset_095(prev):
    return {"step": 95, "value": prev["value"] + 95}


@asset(ins={"prev": AssetIn(key="asset_095")})
def asset_096(prev):
    return {"step": 96, "value": prev["value"] + 96}


@asset(ins={"prev": AssetIn(key="asset_096")})
def asset_097(prev):
    return {"step": 97, "value": prev["value"] + 97}


@asset(ins={"prev": AssetIn(key="asset_097")})
def asset_098(prev):
    return {"step": 98, "value": prev["value"] + 98}


@asset(ins={"prev": AssetIn(key="asset_098")})
def asset_099(prev):
    return {"step": 99, "value": prev["value"] + 99}


if __name__ == "__main__":
    all_assets = [
        asset_000,
        asset_001,
        asset_002,
        asset_003,
        asset_004,
        asset_005,
        asset_006,
        asset_007,
        asset_008,
        asset_009,
        asset_010,
        asset_011,
        asset_012,
        asset_013,
        asset_014,
        asset_015,
        asset_016,
        asset_017,
        asset_018,
        asset_019,
        asset_020,
        asset_021,
        asset_022,
        asset_023,
        asset_024,
        asset_025,
        asset_026,
        asset_027,
        asset_028,
        asset_029,
        asset_030,
        asset_031,
        asset_032,
        asset_033,
        asset_034,
        asset_035,
        asset_036,
        asset_037,
        asset_038,
        asset_039,
        asset_040,
        asset_041,
        asset_042,
        asset_043,
        asset_044,
        asset_045,
        asset_046,
        asset_047,
        asset_048,
        asset_049,
        asset_050,
        asset_051,
        asset_052,
        asset_053,
        asset_054,
        asset_055,
        asset_056,
        asset_057,
        asset_058,
        asset_059,
        asset_060,
        asset_061,
        asset_062,
        asset_063,
        asset_064,
        asset_065,
        asset_066,
        asset_067,
        asset_068,
        asset_069,
        asset_070,
        asset_071,
        asset_072,
        asset_073,
        asset_074,
        asset_075,
        asset_076,
        asset_077,
        asset_078,
        asset_079,
        asset_080,
        asset_081,
        asset_082,
        asset_083,
        asset_084,
        asset_085,
        asset_086,
        asset_087,
        asset_088,
        asset_089,
        asset_090,
        asset_091,
        asset_092,
        asset_093,
        asset_094,
        asset_095,
        asset_096,
        asset_097,
        asset_098,
        asset_099,
    ]
    t0 = time.perf_counter()
    result = materialize(all_assets)
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 100,
                "success": result.success,
            },
            indent=2,
        )
    )
