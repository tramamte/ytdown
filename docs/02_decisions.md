# 02 결정 기록

개발 중 내려진 결정과 그 이유. PO 결정은 **PO**, 개발 중 PM 판정은 **R번호**로 표기한다. 근거 자료는 `01_spikes.md`.

## PO 결정

| 주제 | 결정 |
| --- | --- |
| 미결사항 6건 (착수 전) | 로직은 Rust 커맨드, macOS 격리 속성 제거, Ubuntu 단축키 X11 한정, 동시 다운로드 상한 3 + 대기열, 오프라인 시 기존 바이너리 사용, 재생목록은 모달 확인 |
| ffmpeg 배포처 | macOS는 osxexperts.net(URL·해시 고정), Windows/Ubuntu는 BtbN GPL 정적 빌드 |
| 기본 포맷 | 영상 컨테이너·음성 포맷 기본값 "원본"(재인코딩 없음). 기존 settings.json 값은 유지 |
| MP4 호환성 | 최고 화질 유지(AV1/Opus 가능). 호환이 필요하면 영상 코덱 H.264, 영상 음성 코덱 AAC 선택 |
| 업데이트 | 시작 시 확인은 입력을 막지 않고 다음 실행 시 적용. 누락된 의존성은 즉시 설치. 설정의 Update 버튼은 즉시 적용(입력 잠김 허용) |
| yt-dlp 빌드 | onedir 빌드(zip을 `bin/yt-dlp/`에 풀어 폴더째 교체; 기존 onefile 설치는 첫 실행 시 자동 전환). 실측(macOS arm64): `--version` onefile 매번 6.0~6.6초 → onedir 설치 후 첫 실행 6.1~6.4초, 이후 0.18초. 다운로드 시작→첫 진행률 7.5초 → 2.6초. 시작 시 존재만 확인, 동작 여부는 다운로드 시점에 판단 |
| 화면 | URL 입력창 하나 + Audio only 스위치, 420×420 고정 창(PO 조정), 진행 중 스트림 단계 표시, 완료 시 막대 숨김·제목 흐리게·용량 표시 |
| 단축키 | 전역 기본 Ctrl+Option+Y / Ctrl+Alt+Y(키 녹화로 변경), 설정 열기 Ctrl+,(macOS ⌘,), 저장 ⌘S/Ctrl+S, 닫기 Esc(⌘W는 메뉴 수정이 필요해 제외), Audio only 토글 ⌘⇧A/Ctrl+Shift+A, Shift+Enter는 1회 음성 |
| 아이콘·색 | 앱 아이콘(남색·주황)을 헤더 로고로 사용. 포커스 링·스위치 등 강조색은 YouTube에 맞춰 빨간색 유지. 아이콘 원본 2종: `app-icon.png`(투명 테두리, Windows·Ubuntu·앱 내 로고), `app-icon-mac.png`(꽉 찬 정사각형, macOS가 모양을 씌움) |
| 영상 속 음성 | 코덱 선호, 최대 비트레이트, 재인코딩 설정 분리. 기존 음성 설정은 "Audio only" 전용 |
| 재생목록 묶음 표시 | "전체" 선택 시 재생목록 행 하나 + 항목별 하위 행. 실패 항목은 건너뛰고 계속(yt-dlp 기본 동작), 일부 실패면 완료 + "n of N failed". 항목별 취소는 미지원: 재생목록은 yt-dlp 프로세스 하나가 항목을 차례로 받으므로 한 항목만 멈출 수 없고, 동시 다운로드 상한에도 1개로 계산됨 |

## 판정 (PM)

| # | 판정 | 이유 |
| --- | --- | --- |
| R1 | 설정은 store 플러그인 대신 appConfigDir/settings.json | 요구사항(명시적 경로) 충족, 플러그인 하나 감소 |
| R2 | yt-dlp에 `--progress` | `--print`가 `--quiet`를 함축해 진행률이 사라짐 |
| R3 | 전역 단축키는 UI가 시작 시 등록, 실패는 안내 | Wayland 등 등록 실패를 사용자에게 보여 주기 위함 |
| R4 | Windows 하위 프로세스는 CREATE_NO_WINDOW | GUI 앱에서 콘솔 창 깜빡임 방지 |
| R6 | 서명은 시크릿이 있을 때만, 없으면 미서명 빌드 | 인증서는 PO 준비 사항 |
| R7 | 크레이트 이름 `ytdown` | 초기에 바꾸는 편이 저렴 |
| R10 | macOS ffmpeg는 URL·해시 고정 | 공식·안정 배포처 부재. 사이트 변경 시 앱 릴리스 필요 |
| R11 | Deno를 세 번째 관리 의존성으로 추가, `--js-runtimes deno:<경로>` | yt-dlp의 YouTube 추출에 필요, GUI 앱은 PATH가 최소라 경로 명시 |
| R13 | 의존성은 스트리밍으로 받으며 해시 계산 | Windows ffmpeg 압축 파일 약 170MB |
| R16 | 설정값 허용 목록 검증 | 설정 문자열이 yt-dlp 인자로 쓰이므로 옵션 주입 차단 |
| R18 | Deno 체크섬은 첫 64자리 hex 토큰 | Windows 체크섬 파일 형식 차이 대비 |
| R19 | Windows 취소 시 임시 파일 정리는 최선 노력 | 잠긴 파일이 남을 수 있음 |
| R20 | yt-dlp에 `--encoding utf-8` | Windows 파이프에서 비ASCII 제목 깨짐 방지 |
| R25 | 취소 시 프로세스 트리 종료(유닉스 SIGTERM→SIGKILL, Windows taskkill), 의존성 다운로드 타임아웃, CSP 적용 | onefile yt-dlp는 실행기+자식 2개 프로세스라 실행기만 죽이면 다운로드가 계속됨(실측 확인) |
| R27 | 설정 필드 추가 시 serde 기본값 필수 | 기존 settings.json이 통째로 초기화되는 것을 방지 |
| R29 | Update 버튼의 즉시 교체가 실패하면 다음 실행 시 적용 | Windows는 실행 중인 exe가 잠김 |
| R32 | yt-dlp의 사용법 오류·파이썬 오류는 "시작은 됨"으로 분류, 재설치 대기 중 취소 가능 | 호환되지 않는 yt-dlp 업데이트 시 매번 재설치하는 것을 방지 |
| R33 | 영상 음성 재인코딩은 MP4·MKV 컨테이너만 | AAC는 WebM에 담을 수 없고 "원본"은 WebM일 수 있음 |
| R34 | 다운로드가 진행·대기 중이면 Update 버튼도 yt-dlp를 즉시 교체하지 않고 다음 실행 시 적용. 교체 도중 비정상 종료로 `.old`만 남으면 시작 시 복구 | onedir yt-dlp는 실행 중에도 `_internal/` 파일을 읽으므로 폴더 교체 시 진행 중 다운로드가 깨질 수 있음 |
| R35 | 모든 다운로드에 `--ignore-config` 적용 | 사용자의 yt-dlp 설정 파일(`-o`, `-f`, `--abort-on-error` 등)이 앱의 출력 형식·포맷 선택·오류 처리를 바꾸지 못하게 함 |

폐기된 판정: R28(Update 버튼도 다음 실행 시 적용 → PO가 즉시 적용으로 변경), R30(수동 업데이트 중 입력 유지 → 설정 창이 모달이라 불필요, 되돌림).

## 알려진 한계 (보류)

- 순수 재생목록 URL(`/playlist?list=`)에서 "This video only"를 골라도 전체가 받아짐
- 999,950~999,999 B는 "1000.0 KB"로 표시
- 단축키 녹화 시 global-hotkey가 모르는 키(IntlBackslash 등)는 저장할 때 오류로 안내
- 완료 제목 색(`--faint`)은 WCAG AA 대비 미만(PO 선택)
