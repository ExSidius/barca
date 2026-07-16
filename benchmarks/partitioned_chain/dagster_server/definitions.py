import hashlib
import os
from dagster import asset, AssetIn, Definitions, define_asset_job, multiprocess_executor

# Matches barca's pool_size and prefect's max_workers for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


@asset(name="fetch_T000")
def fetch_T000():
    return {"ticker": "T000", "price": len("T000") * 10 + hash("T000") % 100}


@asset(name="fetch_T001")
def fetch_T001():
    return {"ticker": "T001", "price": len("T001") * 10 + hash("T001") % 100}


@asset(name="fetch_T002")
def fetch_T002():
    return {"ticker": "T002", "price": len("T002") * 10 + hash("T002") % 100}


@asset(name="fetch_T003")
def fetch_T003():
    return {"ticker": "T003", "price": len("T003") * 10 + hash("T003") % 100}


@asset(name="fetch_T004")
def fetch_T004():
    return {"ticker": "T004", "price": len("T004") * 10 + hash("T004") % 100}


@asset(name="fetch_T005")
def fetch_T005():
    return {"ticker": "T005", "price": len("T005") * 10 + hash("T005") % 100}


@asset(name="fetch_T006")
def fetch_T006():
    return {"ticker": "T006", "price": len("T006") * 10 + hash("T006") % 100}


@asset(name="fetch_T007")
def fetch_T007():
    return {"ticker": "T007", "price": len("T007") * 10 + hash("T007") % 100}


@asset(name="fetch_T008")
def fetch_T008():
    return {"ticker": "T008", "price": len("T008") * 10 + hash("T008") % 100}


@asset(name="fetch_T009")
def fetch_T009():
    return {"ticker": "T009", "price": len("T009") * 10 + hash("T009") % 100}


@asset(name="fetch_T010")
def fetch_T010():
    return {"ticker": "T010", "price": len("T010") * 10 + hash("T010") % 100}


@asset(name="fetch_T011")
def fetch_T011():
    return {"ticker": "T011", "price": len("T011") * 10 + hash("T011") % 100}


@asset(name="fetch_T012")
def fetch_T012():
    return {"ticker": "T012", "price": len("T012") * 10 + hash("T012") % 100}


@asset(name="fetch_T013")
def fetch_T013():
    return {"ticker": "T013", "price": len("T013") * 10 + hash("T013") % 100}


@asset(name="fetch_T014")
def fetch_T014():
    return {"ticker": "T014", "price": len("T014") * 10 + hash("T014") % 100}


@asset(name="fetch_T015")
def fetch_T015():
    return {"ticker": "T015", "price": len("T015") * 10 + hash("T015") % 100}


@asset(name="fetch_T016")
def fetch_T016():
    return {"ticker": "T016", "price": len("T016") * 10 + hash("T016") % 100}


@asset(name="fetch_T017")
def fetch_T017():
    return {"ticker": "T017", "price": len("T017") * 10 + hash("T017") % 100}


@asset(name="fetch_T018")
def fetch_T018():
    return {"ticker": "T018", "price": len("T018") * 10 + hash("T018") % 100}


@asset(name="fetch_T019")
def fetch_T019():
    return {"ticker": "T019", "price": len("T019") * 10 + hash("T019") % 100}


@asset(name="fetch_T020")
def fetch_T020():
    return {"ticker": "T020", "price": len("T020") * 10 + hash("T020") % 100}


@asset(name="fetch_T021")
def fetch_T021():
    return {"ticker": "T021", "price": len("T021") * 10 + hash("T021") % 100}


@asset(name="fetch_T022")
def fetch_T022():
    return {"ticker": "T022", "price": len("T022") * 10 + hash("T022") % 100}


@asset(name="fetch_T023")
def fetch_T023():
    return {"ticker": "T023", "price": len("T023") * 10 + hash("T023") % 100}


@asset(name="fetch_T024")
def fetch_T024():
    return {"ticker": "T024", "price": len("T024") * 10 + hash("T024") % 100}


@asset(name="fetch_T025")
def fetch_T025():
    return {"ticker": "T025", "price": len("T025") * 10 + hash("T025") % 100}


@asset(name="fetch_T026")
def fetch_T026():
    return {"ticker": "T026", "price": len("T026") * 10 + hash("T026") % 100}


@asset(name="fetch_T027")
def fetch_T027():
    return {"ticker": "T027", "price": len("T027") * 10 + hash("T027") % 100}


@asset(name="fetch_T028")
def fetch_T028():
    return {"ticker": "T028", "price": len("T028") * 10 + hash("T028") % 100}


@asset(name="fetch_T029")
def fetch_T029():
    return {"ticker": "T029", "price": len("T029") * 10 + hash("T029") % 100}


@asset(name="fetch_T030")
def fetch_T030():
    return {"ticker": "T030", "price": len("T030") * 10 + hash("T030") % 100}


@asset(name="fetch_T031")
def fetch_T031():
    return {"ticker": "T031", "price": len("T031") * 10 + hash("T031") % 100}


@asset(name="fetch_T032")
def fetch_T032():
    return {"ticker": "T032", "price": len("T032") * 10 + hash("T032") % 100}


@asset(name="fetch_T033")
def fetch_T033():
    return {"ticker": "T033", "price": len("T033") * 10 + hash("T033") % 100}


@asset(name="fetch_T034")
def fetch_T034():
    return {"ticker": "T034", "price": len("T034") * 10 + hash("T034") % 100}


@asset(name="fetch_T035")
def fetch_T035():
    return {"ticker": "T035", "price": len("T035") * 10 + hash("T035") % 100}


@asset(name="fetch_T036")
def fetch_T036():
    return {"ticker": "T036", "price": len("T036") * 10 + hash("T036") % 100}


@asset(name="fetch_T037")
def fetch_T037():
    return {"ticker": "T037", "price": len("T037") * 10 + hash("T037") % 100}


@asset(name="fetch_T038")
def fetch_T038():
    return {"ticker": "T038", "price": len("T038") * 10 + hash("T038") % 100}


@asset(name="fetch_T039")
def fetch_T039():
    return {"ticker": "T039", "price": len("T039") * 10 + hash("T039") % 100}


@asset(name="fetch_T040")
def fetch_T040():
    return {"ticker": "T040", "price": len("T040") * 10 + hash("T040") % 100}


@asset(name="fetch_T041")
def fetch_T041():
    return {"ticker": "T041", "price": len("T041") * 10 + hash("T041") % 100}


@asset(name="fetch_T042")
def fetch_T042():
    return {"ticker": "T042", "price": len("T042") * 10 + hash("T042") % 100}


@asset(name="fetch_T043")
def fetch_T043():
    return {"ticker": "T043", "price": len("T043") * 10 + hash("T043") % 100}


@asset(name="fetch_T044")
def fetch_T044():
    return {"ticker": "T044", "price": len("T044") * 10 + hash("T044") % 100}


@asset(name="fetch_T045")
def fetch_T045():
    return {"ticker": "T045", "price": len("T045") * 10 + hash("T045") % 100}


@asset(name="fetch_T046")
def fetch_T046():
    return {"ticker": "T046", "price": len("T046") * 10 + hash("T046") % 100}


@asset(name="fetch_T047")
def fetch_T047():
    return {"ticker": "T047", "price": len("T047") * 10 + hash("T047") % 100}


@asset(name="fetch_T048")
def fetch_T048():
    return {"ticker": "T048", "price": len("T048") * 10 + hash("T048") % 100}


@asset(name="fetch_T049")
def fetch_T049():
    return {"ticker": "T049", "price": len("T049") * 10 + hash("T049") % 100}


@asset(name="normalize_T000", ins={"data": AssetIn(key="fetch_T000")})
def normalize_T000(data):
    return {"ticker": "T000", "normalized": data["price"] / 100.0}


@asset(name="normalize_T001", ins={"data": AssetIn(key="fetch_T001")})
def normalize_T001(data):
    return {"ticker": "T001", "normalized": data["price"] / 100.0}


@asset(name="normalize_T002", ins={"data": AssetIn(key="fetch_T002")})
def normalize_T002(data):
    return {"ticker": "T002", "normalized": data["price"] / 100.0}


@asset(name="normalize_T003", ins={"data": AssetIn(key="fetch_T003")})
def normalize_T003(data):
    return {"ticker": "T003", "normalized": data["price"] / 100.0}


@asset(name="normalize_T004", ins={"data": AssetIn(key="fetch_T004")})
def normalize_T004(data):
    return {"ticker": "T004", "normalized": data["price"] / 100.0}


@asset(name="normalize_T005", ins={"data": AssetIn(key="fetch_T005")})
def normalize_T005(data):
    return {"ticker": "T005", "normalized": data["price"] / 100.0}


@asset(name="normalize_T006", ins={"data": AssetIn(key="fetch_T006")})
def normalize_T006(data):
    return {"ticker": "T006", "normalized": data["price"] / 100.0}


@asset(name="normalize_T007", ins={"data": AssetIn(key="fetch_T007")})
def normalize_T007(data):
    return {"ticker": "T007", "normalized": data["price"] / 100.0}


@asset(name="normalize_T008", ins={"data": AssetIn(key="fetch_T008")})
def normalize_T008(data):
    return {"ticker": "T008", "normalized": data["price"] / 100.0}


@asset(name="normalize_T009", ins={"data": AssetIn(key="fetch_T009")})
def normalize_T009(data):
    return {"ticker": "T009", "normalized": data["price"] / 100.0}


@asset(name="normalize_T010", ins={"data": AssetIn(key="fetch_T010")})
def normalize_T010(data):
    return {"ticker": "T010", "normalized": data["price"] / 100.0}


@asset(name="normalize_T011", ins={"data": AssetIn(key="fetch_T011")})
def normalize_T011(data):
    return {"ticker": "T011", "normalized": data["price"] / 100.0}


@asset(name="normalize_T012", ins={"data": AssetIn(key="fetch_T012")})
def normalize_T012(data):
    return {"ticker": "T012", "normalized": data["price"] / 100.0}


@asset(name="normalize_T013", ins={"data": AssetIn(key="fetch_T013")})
def normalize_T013(data):
    return {"ticker": "T013", "normalized": data["price"] / 100.0}


@asset(name="normalize_T014", ins={"data": AssetIn(key="fetch_T014")})
def normalize_T014(data):
    return {"ticker": "T014", "normalized": data["price"] / 100.0}


@asset(name="normalize_T015", ins={"data": AssetIn(key="fetch_T015")})
def normalize_T015(data):
    return {"ticker": "T015", "normalized": data["price"] / 100.0}


@asset(name="normalize_T016", ins={"data": AssetIn(key="fetch_T016")})
def normalize_T016(data):
    return {"ticker": "T016", "normalized": data["price"] / 100.0}


@asset(name="normalize_T017", ins={"data": AssetIn(key="fetch_T017")})
def normalize_T017(data):
    return {"ticker": "T017", "normalized": data["price"] / 100.0}


@asset(name="normalize_T018", ins={"data": AssetIn(key="fetch_T018")})
def normalize_T018(data):
    return {"ticker": "T018", "normalized": data["price"] / 100.0}


@asset(name="normalize_T019", ins={"data": AssetIn(key="fetch_T019")})
def normalize_T019(data):
    return {"ticker": "T019", "normalized": data["price"] / 100.0}


@asset(name="normalize_T020", ins={"data": AssetIn(key="fetch_T020")})
def normalize_T020(data):
    return {"ticker": "T020", "normalized": data["price"] / 100.0}


@asset(name="normalize_T021", ins={"data": AssetIn(key="fetch_T021")})
def normalize_T021(data):
    return {"ticker": "T021", "normalized": data["price"] / 100.0}


@asset(name="normalize_T022", ins={"data": AssetIn(key="fetch_T022")})
def normalize_T022(data):
    return {"ticker": "T022", "normalized": data["price"] / 100.0}


@asset(name="normalize_T023", ins={"data": AssetIn(key="fetch_T023")})
def normalize_T023(data):
    return {"ticker": "T023", "normalized": data["price"] / 100.0}


@asset(name="normalize_T024", ins={"data": AssetIn(key="fetch_T024")})
def normalize_T024(data):
    return {"ticker": "T024", "normalized": data["price"] / 100.0}


@asset(name="normalize_T025", ins={"data": AssetIn(key="fetch_T025")})
def normalize_T025(data):
    return {"ticker": "T025", "normalized": data["price"] / 100.0}


@asset(name="normalize_T026", ins={"data": AssetIn(key="fetch_T026")})
def normalize_T026(data):
    return {"ticker": "T026", "normalized": data["price"] / 100.0}


@asset(name="normalize_T027", ins={"data": AssetIn(key="fetch_T027")})
def normalize_T027(data):
    return {"ticker": "T027", "normalized": data["price"] / 100.0}


@asset(name="normalize_T028", ins={"data": AssetIn(key="fetch_T028")})
def normalize_T028(data):
    return {"ticker": "T028", "normalized": data["price"] / 100.0}


@asset(name="normalize_T029", ins={"data": AssetIn(key="fetch_T029")})
def normalize_T029(data):
    return {"ticker": "T029", "normalized": data["price"] / 100.0}


@asset(name="normalize_T030", ins={"data": AssetIn(key="fetch_T030")})
def normalize_T030(data):
    return {"ticker": "T030", "normalized": data["price"] / 100.0}


@asset(name="normalize_T031", ins={"data": AssetIn(key="fetch_T031")})
def normalize_T031(data):
    return {"ticker": "T031", "normalized": data["price"] / 100.0}


@asset(name="normalize_T032", ins={"data": AssetIn(key="fetch_T032")})
def normalize_T032(data):
    return {"ticker": "T032", "normalized": data["price"] / 100.0}


@asset(name="normalize_T033", ins={"data": AssetIn(key="fetch_T033")})
def normalize_T033(data):
    return {"ticker": "T033", "normalized": data["price"] / 100.0}


@asset(name="normalize_T034", ins={"data": AssetIn(key="fetch_T034")})
def normalize_T034(data):
    return {"ticker": "T034", "normalized": data["price"] / 100.0}


@asset(name="normalize_T035", ins={"data": AssetIn(key="fetch_T035")})
def normalize_T035(data):
    return {"ticker": "T035", "normalized": data["price"] / 100.0}


@asset(name="normalize_T036", ins={"data": AssetIn(key="fetch_T036")})
def normalize_T036(data):
    return {"ticker": "T036", "normalized": data["price"] / 100.0}


@asset(name="normalize_T037", ins={"data": AssetIn(key="fetch_T037")})
def normalize_T037(data):
    return {"ticker": "T037", "normalized": data["price"] / 100.0}


@asset(name="normalize_T038", ins={"data": AssetIn(key="fetch_T038")})
def normalize_T038(data):
    return {"ticker": "T038", "normalized": data["price"] / 100.0}


@asset(name="normalize_T039", ins={"data": AssetIn(key="fetch_T039")})
def normalize_T039(data):
    return {"ticker": "T039", "normalized": data["price"] / 100.0}


@asset(name="normalize_T040", ins={"data": AssetIn(key="fetch_T040")})
def normalize_T040(data):
    return {"ticker": "T040", "normalized": data["price"] / 100.0}


@asset(name="normalize_T041", ins={"data": AssetIn(key="fetch_T041")})
def normalize_T041(data):
    return {"ticker": "T041", "normalized": data["price"] / 100.0}


@asset(name="normalize_T042", ins={"data": AssetIn(key="fetch_T042")})
def normalize_T042(data):
    return {"ticker": "T042", "normalized": data["price"] / 100.0}


@asset(name="normalize_T043", ins={"data": AssetIn(key="fetch_T043")})
def normalize_T043(data):
    return {"ticker": "T043", "normalized": data["price"] / 100.0}


@asset(name="normalize_T044", ins={"data": AssetIn(key="fetch_T044")})
def normalize_T044(data):
    return {"ticker": "T044", "normalized": data["price"] / 100.0}


@asset(name="normalize_T045", ins={"data": AssetIn(key="fetch_T045")})
def normalize_T045(data):
    return {"ticker": "T045", "normalized": data["price"] / 100.0}


@asset(name="normalize_T046", ins={"data": AssetIn(key="fetch_T046")})
def normalize_T046(data):
    return {"ticker": "T046", "normalized": data["price"] / 100.0}


@asset(name="normalize_T047", ins={"data": AssetIn(key="fetch_T047")})
def normalize_T047(data):
    return {"ticker": "T047", "normalized": data["price"] / 100.0}


@asset(name="normalize_T048", ins={"data": AssetIn(key="fetch_T048")})
def normalize_T048(data):
    return {"ticker": "T048", "normalized": data["price"] / 100.0}


@asset(name="normalize_T049", ins={"data": AssetIn(key="fetch_T049")})
def normalize_T049(data):
    return {"ticker": "T049", "normalized": data["price"] / 100.0}


@asset(name="score_T000", ins={"data": AssetIn(key="normalize_T000")})
def score_T000(data):
    h = hashlib.md5(f"T000:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T000", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T001", ins={"data": AssetIn(key="normalize_T001")})
def score_T001(data):
    h = hashlib.md5(f"T001:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T001", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T002", ins={"data": AssetIn(key="normalize_T002")})
def score_T002(data):
    h = hashlib.md5(f"T002:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T002", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T003", ins={"data": AssetIn(key="normalize_T003")})
def score_T003(data):
    h = hashlib.md5(f"T003:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T003", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T004", ins={"data": AssetIn(key="normalize_T004")})
def score_T004(data):
    h = hashlib.md5(f"T004:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T004", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T005", ins={"data": AssetIn(key="normalize_T005")})
def score_T005(data):
    h = hashlib.md5(f"T005:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T005", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T006", ins={"data": AssetIn(key="normalize_T006")})
def score_T006(data):
    h = hashlib.md5(f"T006:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T006", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T007", ins={"data": AssetIn(key="normalize_T007")})
def score_T007(data):
    h = hashlib.md5(f"T007:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T007", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T008", ins={"data": AssetIn(key="normalize_T008")})
def score_T008(data):
    h = hashlib.md5(f"T008:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T008", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T009", ins={"data": AssetIn(key="normalize_T009")})
def score_T009(data):
    h = hashlib.md5(f"T009:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T009", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T010", ins={"data": AssetIn(key="normalize_T010")})
def score_T010(data):
    h = hashlib.md5(f"T010:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T010", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T011", ins={"data": AssetIn(key="normalize_T011")})
def score_T011(data):
    h = hashlib.md5(f"T011:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T011", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T012", ins={"data": AssetIn(key="normalize_T012")})
def score_T012(data):
    h = hashlib.md5(f"T012:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T012", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T013", ins={"data": AssetIn(key="normalize_T013")})
def score_T013(data):
    h = hashlib.md5(f"T013:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T013", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T014", ins={"data": AssetIn(key="normalize_T014")})
def score_T014(data):
    h = hashlib.md5(f"T014:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T014", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T015", ins={"data": AssetIn(key="normalize_T015")})
def score_T015(data):
    h = hashlib.md5(f"T015:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T015", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T016", ins={"data": AssetIn(key="normalize_T016")})
def score_T016(data):
    h = hashlib.md5(f"T016:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T016", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T017", ins={"data": AssetIn(key="normalize_T017")})
def score_T017(data):
    h = hashlib.md5(f"T017:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T017", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T018", ins={"data": AssetIn(key="normalize_T018")})
def score_T018(data):
    h = hashlib.md5(f"T018:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T018", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T019", ins={"data": AssetIn(key="normalize_T019")})
def score_T019(data):
    h = hashlib.md5(f"T019:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T019", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T020", ins={"data": AssetIn(key="normalize_T020")})
def score_T020(data):
    h = hashlib.md5(f"T020:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T020", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T021", ins={"data": AssetIn(key="normalize_T021")})
def score_T021(data):
    h = hashlib.md5(f"T021:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T021", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T022", ins={"data": AssetIn(key="normalize_T022")})
def score_T022(data):
    h = hashlib.md5(f"T022:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T022", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T023", ins={"data": AssetIn(key="normalize_T023")})
def score_T023(data):
    h = hashlib.md5(f"T023:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T023", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T024", ins={"data": AssetIn(key="normalize_T024")})
def score_T024(data):
    h = hashlib.md5(f"T024:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T024", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T025", ins={"data": AssetIn(key="normalize_T025")})
def score_T025(data):
    h = hashlib.md5(f"T025:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T025", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T026", ins={"data": AssetIn(key="normalize_T026")})
def score_T026(data):
    h = hashlib.md5(f"T026:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T026", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T027", ins={"data": AssetIn(key="normalize_T027")})
def score_T027(data):
    h = hashlib.md5(f"T027:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T027", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T028", ins={"data": AssetIn(key="normalize_T028")})
def score_T028(data):
    h = hashlib.md5(f"T028:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T028", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T029", ins={"data": AssetIn(key="normalize_T029")})
def score_T029(data):
    h = hashlib.md5(f"T029:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T029", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T030", ins={"data": AssetIn(key="normalize_T030")})
def score_T030(data):
    h = hashlib.md5(f"T030:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T030", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T031", ins={"data": AssetIn(key="normalize_T031")})
def score_T031(data):
    h = hashlib.md5(f"T031:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T031", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T032", ins={"data": AssetIn(key="normalize_T032")})
def score_T032(data):
    h = hashlib.md5(f"T032:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T032", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T033", ins={"data": AssetIn(key="normalize_T033")})
def score_T033(data):
    h = hashlib.md5(f"T033:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T033", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T034", ins={"data": AssetIn(key="normalize_T034")})
def score_T034(data):
    h = hashlib.md5(f"T034:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T034", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T035", ins={"data": AssetIn(key="normalize_T035")})
def score_T035(data):
    h = hashlib.md5(f"T035:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T035", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T036", ins={"data": AssetIn(key="normalize_T036")})
def score_T036(data):
    h = hashlib.md5(f"T036:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T036", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T037", ins={"data": AssetIn(key="normalize_T037")})
def score_T037(data):
    h = hashlib.md5(f"T037:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T037", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T038", ins={"data": AssetIn(key="normalize_T038")})
def score_T038(data):
    h = hashlib.md5(f"T038:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T038", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T039", ins={"data": AssetIn(key="normalize_T039")})
def score_T039(data):
    h = hashlib.md5(f"T039:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T039", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T040", ins={"data": AssetIn(key="normalize_T040")})
def score_T040(data):
    h = hashlib.md5(f"T040:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T040", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T041", ins={"data": AssetIn(key="normalize_T041")})
def score_T041(data):
    h = hashlib.md5(f"T041:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T041", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T042", ins={"data": AssetIn(key="normalize_T042")})
def score_T042(data):
    h = hashlib.md5(f"T042:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T042", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T043", ins={"data": AssetIn(key="normalize_T043")})
def score_T043(data):
    h = hashlib.md5(f"T043:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T043", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T044", ins={"data": AssetIn(key="normalize_T044")})
def score_T044(data):
    h = hashlib.md5(f"T044:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T044", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T045", ins={"data": AssetIn(key="normalize_T045")})
def score_T045(data):
    h = hashlib.md5(f"T045:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T045", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T046", ins={"data": AssetIn(key="normalize_T046")})
def score_T046(data):
    h = hashlib.md5(f"T046:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T046", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T047", ins={"data": AssetIn(key="normalize_T047")})
def score_T047(data):
    h = hashlib.md5(f"T047:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T047", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T048", ins={"data": AssetIn(key="normalize_T048")})
def score_T048(data):
    h = hashlib.md5(f"T048:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T048", "score": int(h[:8], 16) / 2**32}


@asset(name="score_T049", ins={"data": AssetIn(key="normalize_T049")})
def score_T049(data):
    h = hashlib.md5(f"T049:{data['normalized']}".encode()).hexdigest()
    return {"ticker": "T049", "score": int(h[:8], 16) / 2**32}


ALL_ASSETS = [
    fetch_T000,
    fetch_T001,
    fetch_T002,
    fetch_T003,
    fetch_T004,
    fetch_T005,
    fetch_T006,
    fetch_T007,
    fetch_T008,
    fetch_T009,
    fetch_T010,
    fetch_T011,
    fetch_T012,
    fetch_T013,
    fetch_T014,
    fetch_T015,
    fetch_T016,
    fetch_T017,
    fetch_T018,
    fetch_T019,
    fetch_T020,
    fetch_T021,
    fetch_T022,
    fetch_T023,
    fetch_T024,
    fetch_T025,
    fetch_T026,
    fetch_T027,
    fetch_T028,
    fetch_T029,
    fetch_T030,
    fetch_T031,
    fetch_T032,
    fetch_T033,
    fetch_T034,
    fetch_T035,
    fetch_T036,
    fetch_T037,
    fetch_T038,
    fetch_T039,
    fetch_T040,
    fetch_T041,
    fetch_T042,
    fetch_T043,
    fetch_T044,
    fetch_T045,
    fetch_T046,
    fetch_T047,
    fetch_T048,
    fetch_T049,
    normalize_T000,
    normalize_T001,
    normalize_T002,
    normalize_T003,
    normalize_T004,
    normalize_T005,
    normalize_T006,
    normalize_T007,
    normalize_T008,
    normalize_T009,
    normalize_T010,
    normalize_T011,
    normalize_T012,
    normalize_T013,
    normalize_T014,
    normalize_T015,
    normalize_T016,
    normalize_T017,
    normalize_T018,
    normalize_T019,
    normalize_T020,
    normalize_T021,
    normalize_T022,
    normalize_T023,
    normalize_T024,
    normalize_T025,
    normalize_T026,
    normalize_T027,
    normalize_T028,
    normalize_T029,
    normalize_T030,
    normalize_T031,
    normalize_T032,
    normalize_T033,
    normalize_T034,
    normalize_T035,
    normalize_T036,
    normalize_T037,
    normalize_T038,
    normalize_T039,
    normalize_T040,
    normalize_T041,
    normalize_T042,
    normalize_T043,
    normalize_T044,
    normalize_T045,
    normalize_T046,
    normalize_T047,
    normalize_T048,
    normalize_T049,
    score_T000,
    score_T001,
    score_T002,
    score_T003,
    score_T004,
    score_T005,
    score_T006,
    score_T007,
    score_T008,
    score_T009,
    score_T010,
    score_T011,
    score_T012,
    score_T013,
    score_T014,
    score_T015,
    score_T016,
    score_T017,
    score_T018,
    score_T019,
    score_T020,
    score_T021,
    score_T022,
    score_T023,
    score_T024,
    score_T025,
    score_T026,
    score_T027,
    score_T028,
    score_T029,
    score_T030,
    score_T031,
    score_T032,
    score_T033,
    score_T034,
    score_T035,
    score_T036,
    score_T037,
    score_T038,
    score_T039,
    score_T040,
    score_T041,
    score_T042,
    score_T043,
    score_T044,
    score_T045,
    score_T046,
    score_T047,
    score_T048,
    score_T049,
]
job = define_asset_job(
    "partition_chain_job",
    selection="*",
    executor_def=multiprocess_executor.configured({"max_concurrent": BENCH_WORKERS}),
)
defs = Definitions(assets=ALL_ASSETS, jobs=[job])
