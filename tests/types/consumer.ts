// Compile the installed package's public declarations with a strict downstream TypeScript project.
import { Kraken, type LearningPolicy, type LearnResult, type ResearchInput } from 'n2-kraken';

const policy: LearningPolicy = {
  id: 'consumer-review-v1',
  screen(input, signal) {
    signal.throwIfAborted();
    return input.facts.map(({ index }) => index);
  },
};

async function checkConsumer(): Promise<void> {
  const engine = new Kraken({ dbPath: ':memory:', project: 'consumer', agent: 'reader' });
  const request: ResearchInput = { requestId: 'consumer-request', topic: 'Rust memory safety' };
  const research = await engine.research(request);
  const learned: LearnResult = await engine.learn(research.requestId, { policy });
  const created: boolean | null | undefined = engine.getLearning(request.requestId)?.insights[0]
    ?.created;
  const count: number = learned.newInsights;
  void created;
  void count;
  await engine.close();
}

void checkConsumer;
