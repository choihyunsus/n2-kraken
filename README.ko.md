# N2 Kraken

[English](README.md)

로컬 AI 에이전트를 위한 공개 웹 수집·조사 모듈입니다. HTML 읽기, HTTP 가져오기, 설치된 Chrome을 이용한 렌더링, 검색 엔진 순차 조회, 출처별 주장 비교, Soul/Mimir 공통 SQLite DB를 통한 명시적 학습을 제공합니다.

## 설치

Node.js 22 이상이 필요합니다. GitHub 릴리스의 패키지를 설치합니다.

```sh
npm install https://github.com/choihyunsus/n2-kraken/releases/download/v1.0.0/n2-kraken-1.0.0.tgz
```

실행 파일은 `bin/n2-kraken-worker-<platform>-<architecture>`에서 선택됩니다. Windows 파일에는 `.exe`가 붙습니다. 직접 만든 실행 파일은 `workerPath`로 지정합니다. 해당 플랫폼용 빌드가 필요하면 rustup의 Rust 1.98.0, C/C++ 빌드 도구, CMake, LLVM/libclang을 설치한 뒤 패키지 폴더에서 실행합니다.

```sh
npm run build:native
```

렌더링과 Google 검색에는 Chrome 설치가 필요합니다. 자동 탐색에 실패하면 `chromePath`를 지정합니다. 선택적 의미 비교는 실행 중인 Ollama의 설치된 임베딩 모델을 이용합니다.

## 라이브러리 사용

```js
const { Kraken } = require('n2-kraken');

const kraken = new Kraken({
  dbPath: './data/mimir.db',
  project: 'my-project',
  agent: 'researcher',
});

try {
  const page = await kraken.fetch({ url: 'https://example.com/' });
  console.log(page.source_url, page.markdown, page.truncated);

  const research = await kraken.research({
    requestId: 'rust-memory-safety-001',
    topic: 'Rust memory safety borrowing',
    config: { max_results: 5, max_crawl_pages: 3 },
  });
  console.log(research.report.summary);

  const learned = await kraken.learn(research.requestId);
  console.log(learned.newInsights, learned.verified);
} finally {
  await kraken.close();
}
```

ESM에서는 `import { Kraken } from 'n2-kraken'`을 사용합니다. TypeScript 타입 선언을 제공합니다. 모든 인스턴스에 `dbPath`, `project`, `agent`를 명시해야 합니다. Soul이 사용하는 `mimir.db`를 지정하면 공통 경험·통찰·태그·추론 저장 계약으로 연동됩니다. 조회와 기록은 해당 프로젝트와 에이전트 범위에서 이루어집니다.

| 메서드 | 기능 |
| --- | --- |
| `extract({html, source_url?, config?}, signal?)` | HTML에서 Markdown과 잘림 정보를 추출 |
| `fetch({url, config?}, signal?)` | 공개 HTTP(S) 문서를 가져오고 최종 리디렉션 URL을 반환 |
| `render({url, format?, max_length?}, signal?)` | Chrome에서 실제 DOM 텍스트 또는 렌더링된 HTML을 추출 |
| `search({query, config?}, signal?)` | 중복 URL을 정리한 검색 결과 반환 |
| `research({requestId, topic, config?, semantic?}, signal?)` | 조사 요약·출처 본문·실패·주장 비교 결과를 기록 |
| `getResearch(requestId)` | 기록된 전체 조사 결과 조회 |
| `listResearch(limit?, offset?)` | 현재 범위의 조사 기록 목록 조회 |
| `learn(requestId, {policy?, signal?}?)` | 출처·미검증 통찰·학습 기록을 하나의 트랜잭션으로 저장 |
| `close()` | 진행 중인 작업을 취소하고 DB 연결 종료 |

`extract`와 `fetch`의 `config`는 `include_links`, `include_images`, UTF-8 바이트 단위 `max_length`를 받습니다. 기본 출력 한도는 50,000바이트입니다. `render`의 기본 형식은 `text`이며 동일한 바이트 한도를 사용합니다.

## 조사 설정

| 필드 | 기본값 | 범위 |
| --- | --- | --- |
| `max_results` | 10 | 1–40 |
| `max_crawl_pages` | 5 | 1–20, `max_results` 이하 |
| `max_content_length` | 5,000 | 출처당 50–100,000 UTF-8 바이트 |
| `min_confidence` | 0.6 | 출처 일치 점수 0.5–1.0 |
| `timeout_ms` | 30,000 | 엔진·페이지당 100–120,000밀리초 |
| `total_timeout_ms` | 180,000 | 작업당 100–600,000밀리초 |
| `time_range` | `3m` | 빈 문자열, `1w`, `1m`, `3m`, `6m`, `1y` |
| `engines` | `ddg,bing,naver` | 중복 없는 순서 목록, `google,searxng` 추가 가능 |

검색 엔진은 지정한 순서로 시도하며, 유효한 결과를 얻으면 그 결과를 사용합니다. 날짜 조건은 각 엔진의 지원 단위를 따릅니다. 검색 페이지 구조·순위·접근 조건은 달라질 수 있습니다. Google이 대화형 인증을 요구하면 검색 실패로 보고합니다.

서비스 주소는 호스트에서 설정합니다.

```js
const kraken = new Kraken({
  dbPath: './data/mimir.db', project: 'my-project', agent: 'researcher',
  searxngUrl: 'http://127.0.0.1:8080/',
  ollama: { endpoint: 'http://127.0.0.1:11434', model: 'nomic-embed-text' },
});
```

`engines: ['searxng']`을 선택하면 설정된 검색 서비스를 사용합니다. 조사 입력의 `semantic: true`는 임베딩 비교를 요청합니다. 기본 의미 비교 서비스는 로컬 Ollama의 `nomic-embed-text`입니다. 서비스 불가·잘못된 벡터·크기 초과 상황에서는 키워드 비교로 전환하며, `summary.semantic_mode`에 실제 사용 여부를 기록합니다.

## 학습과 출처

조사 기록에는 실행 파일 해시, 적용된 입력 해시, 전체 보고서 해시, 출처 본문 SHA-256, 최종 URL, 잘림 여부와 수집 실패가 포함됩니다. 같은 요청 ID와 같은 입력의 재호출은 저장된 결과를 돌려줍니다. 다른 입력으로 기존 ID를 사용하면 오류가 발생합니다. 새 조사는 새 ID로 요청합니다.

네이티브 결과의 `Verified` 상태는 서로 다른 URL 세 개 이상, 지정한 점수 이상, 감지된 모순 없음이라는 출처 일치 조건을 뜻합니다. 사실 정확성·발행처 독립성·전재된 문장·정교한 허위 정보는 추가 평가가 필요합니다. 키워드와 모순 판정 규칙은 언어적 범위가 제한적이며 정상적인 기술 설명을 걸러낼 수도 있습니다.

`learn`은 출처를 `pattern` 경험으로 저장하고, 그 출처에 연결된 미검증 추론 결론을 만듭니다. 같은 근거와 같은 주장은 다른 조사 요청에서도 통찰을 재사용합니다. 출처 내용이 바뀌면 이전 근거를 무효화하고 새 근거를 원자적으로 저장합니다. 이전 내용과 충돌하는 오래된 보고서는 다시 조사해야 합니다. 삭제되거나 정리된 출처는 새 근거를 만든 뒤 재사용합니다.

호스트의 `policy.screen`으로 저장 대상을 더 제한할 수 있습니다. 정책을 개정할 때는 새로운 `policy.id`를 사용합니다. 같은 정책 ID로 기존 학습 기록을 복구하면 처음 승인한 선택을 유지합니다.

```js
await kraken.learn('rust-memory-safety-001', {
  policy: {
    id: 'review-v1',
    async screen({ facts, pages }, signal) {
      signal.throwIfAborted();
      return facts.filter(({ fact }) =>
        fact.sources.every(url => pages.some(page => page.url === url && !page.truncated))
      ).map(({ index }) => index);
    },
  },
});
```

비동기 정책 판단은 30초로 제한됩니다. 동기 콜백은 호스트가 신속하게 반환하도록 구현해야 합니다. 실제 기록은 15초 한도의 단일 Mimir 트랜잭션으로 처리합니다. `newSources`와 `newInsights`는 현재 호출에서 새로 추가한 개수입니다. 모듈은 `meta`의 `kraken_schema_version`과 네 개의 범위별 테이블 `kraken_research`, `kraken_sources`, `kraken_claims`, `kraken_learning`을 사용합니다. 조사 원문은 기록당 최대 4 MiB이며 사용량에 따라 누적되므로 운영자가 보관·아카이브 정책을 관리해야 합니다.

## CLI와 MCP

```sh
n2-kraken --help
n2-kraken extract --db ./data/mimir.db --project demo --agent reader < request.json
n2-kraken get --db ./data/mimir.db --project demo --agent reader --id research-001
n2-kraken learn --db ./data/mimir.db --project demo --agent reader --id research-001
n2-kraken mcp --db ./data/mimir.db --project demo --agent reader
```

CLI는 `--input` JSON, `--worker`, `--chrome`, `--searxng`, `--ollama-endpoint`, `--ollama-model`을 지원합니다. `history`에는 `--limit`, `--offset`을 사용할 수 있습니다. 성공 결과는 stdout의 JSON으로, 실패는 stderr와 종료 코드 1로 전달합니다.

stdio MCP는 열 개의 도구를 제공합니다. `kraken_extract`, `kraken_fetch`, `kraken_render`, `kraken_search`, `kraken_research`, `kraken_history`, `kraken_read`, `kraken_source`, `kraken_facts`, `kraken_learn`입니다. 응답 한도는 60,000바이트입니다. 출처 본문은 문자 기준으로 나눠 읽고, 주장에는 해당 출처의 인덱스가 포함됩니다. 미리보기에는 잘림 여부가 표시됩니다. `kraken_read`는 수집 실패를 최대 다섯 개까지 보여주며, 전체 보고서는 라이브러리와 CLI에서 읽습니다.

## 실행 범위

공개 페이지의 목적지와 리디렉션을 검사하고 확인한 공개 IP로 연결합니다. Chrome은 전용 임시 프로필과 공개 네트워크용 프록시를 사용합니다. 명시적으로 설정한 SearXNG·Ollama 주소는 로컬 네트워크 주소를 포함할 수 있으므로 호스트에서 해당 서비스의 접근 권한을 관리해야 합니다.

워커 입력 한도는 16 MiB, 출력 한도는 32 MiB이며 Node.js 프로세스당 최대 두 개가 동시에 실행됩니다. HTTP 본문은 8 MiB, 렌더링 HTML은 UTF-16 코드 단위 200만 개로 제한됩니다. DOM 본문과 프레임의 생략 여부도 결과에 표시합니다. 출처 해시는 반환된 내용에 대응합니다. 웹 스크립트는 샌드박스가 활성화된 Chrome에서 실행됩니다. 브라우저 메모리 사용량은 페이지 동작에 따라 달라집니다. 취소와 종료 처리는 해당 인스턴스가 실행한 프로세스를 대상으로 하며, 호스트 강제 종료 후 임시 파일이 남을 수 있습니다.

## 빌드와 검증

```sh
npm ci
npm run build:native
npm test
cargo +1.98.0 test --manifest-path native/Cargo.toml --all-targets -j 2
cargo +1.98.0 clippy --manifest-path native/Cargo.toml --all-targets -j 2 -- -D warnings
```

테스트는 실제 네이티브 워커 통신, 설정된 SearXNG 전송, 비공개 목적지 차단, SQLite 트랜잭션, 재시작·재시도, 범위 분리, 근거 무효화, 유니코드 잘림, 취소, CLI, 실제 stdio MCP를 포함합니다. 실시간 사이트와 Chrome 검증은 명시적으로 실행하는 Rust ignored 테스트로 구분합니다.

## 라이선스

Kraken 모듈 코드는 MIT입니다. Node.js 연결부가 사용하는 N2 Mimir의 배포 라이선스에는 비상업적 이용 조건과 별도 상업 라이선스가 포함됩니다. 배포·사용 조건은 동봉한 `LICENSE-N2-MIMIR`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES`, 네이티브 의존성 고지를 확인해야 합니다. 네이티브 의존성에는 각각의 라이선스가 적용됩니다.
