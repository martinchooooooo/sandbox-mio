<script lang="ts">
    import { Act, Glossary, GlossaryItem, Page, Term } from "../../book";
    import { HARNESSES, MAKERS, OPEN, TIED, andList } from "./copy";
    import * as act1 from "./act1.scenes";
    import * as act2 from "./act2.scenes";
    import * as act3 from "./act3.scenes";
    import * as act4 from "./act4.scenes";
    import * as act5 from "./act5.scenes";

    // Vocabulary is fixed across this chapter: model = brain, agent = arms, tool = something the
    // arms pick up, chat = where you type, terminal = typed commands, sandbox = pretend computer.
    // TODO(i18n): every act label, title and paragraph below is hard-coded English for now.
</script>

<Act label="Act 1 · At first">
    <Page title="Meet the brain" scene={act1.meetTheBrain}>
        <p>
            An AI <Term>model</Term> is a bit like a brain in a jar. It has read an enormous amount of text, which makes it
            very good at one thing: you give it words, and it gives words back.
        </p>
        <p>That is all it can do. It has no hands, it can't see your screen, and it has no way to touch your computer.</p>
    </Page>

    <Page title="You talk to it in a chat" scene={act1.chatAndReply}>
        <p>
            The <Term>chat</Term> is the window where you type. Your message travels to the brain, the brain thinks, and a
            reply comes back.
        </p>
        <p>Ask it how to change your laptop password, and it will happily tell you.</p>
    </Page>

    <Page title="…but you still do the work" scene={act1.clickAlong}>
        <p>
            The reply is a list of instructions: open this, click that. The brain can't follow them itself. So you do.
        </p>
        <p><Term>Try it:</Term> follow the brain's steps by clicking the glowing buttons on the computer.</p>
    </Page>
</Act>

<Act label="Act 2 · Then">
    <Page title="Someone had a clever idea" scene={act2.armsAppear}>
        <p>Instead of the brain telling <i>you</i> what to do, what if it could tell <i>the computer</i> directly?</p>
        <p>
            So we gave the brain a pair of arms. The arms are called an <Term>agent</Term>. The brain decides what should
            happen, and the agent does the work on your behalf.
        </p>
    </Page>

    <Page title="How do arms use a computer?" scene={act2.mouseVsTerminal}>
        <p>
            You use a computer by pointing and clicking. An agent uses a <Term>terminal</Term> instead: a plain window where
            you type a command and the computer carries it out.
        </p>
        <p>
            It's how programmers drive a computer without a mouse. One typed line can do what took you three clicks.
        </p>
    </Page>

    <Page title="The new loop" scene={act2.theNewLoop}>
        <p>
            You still just chat. But now your message goes to the agent, the agent asks the brain, and the brain's answer
            isn't a list of instructions for you. It's a command for the arms.
        </p>
        <p>The arms run it, show the brain what happened, and you get told when it's done.</p>
    </Page>
</Act>

<Act label="Act 3 · Out in the world">
    <Page title="There are many brains" scene={act3.manyBrains}>
        <p>
            There isn't just one AI <Term>model</Term>. Many companies build their own brains and give them names.
            {MAKERS.slice(0, 3).map((m) => `${m.name} makes ${andList(m.models)}.`).join(" ")}
            {andList(MAKERS.slice(3).map((m) => m.name))} make their own too, and new ones arrive every few months.
        </p>
        <p>
            Here each brain wears its maker's colours. They all work the same way: words in, words out. But some are
            quicker, some think more carefully, and some cost less to use, so people pick different brains for different
            jobs.
        </p>
    </Page>

    <Page title="The arms go by many names" scene={act3.manyNames}>
        <p>
            Out in the world you'll hear the arms called an <i>agent</i>, an <i>agent harness</i>, a <i>coding agent</i>,
            or just a <i>harness</i>. Everyone is still inventing their own words. In this story we call them the
            <Term>agent</Term>.
        </p>
        <p>
            {andList(HARNESSES.map((h) => h.name))} are all agent harnesses: ready-made sets of arms. Some only work with
            one company's brains: {TIED.name} can only be used with {TIED.onlyFrom}'s models. Others, like {OPEN.name},
            let you plug in whichever brain you like.
        </p>
    </Page>

    <Page title="The arms carry tools" scene={act3.armsCarryTools}>
        <p>
            An agent doesn't do everything with its bare hands. It carries <Term>tools</Term>, and each tool does one job:
            run a terminal command, read a file, edit a file, search the web. The brain decides which tool is needed; the
            arms pick it up and use it.
        </p>
        <p>
            Every agent harness comes with its own toolbox. Some carry a handful of tools, some carry dozens, and many let
            you add more. Two agents with the same brain can behave very differently just because of what's in their
            toolbox.
        </p>
    </Page>

    <Page title="Why it's hard to compare" scene={act3.hardToCompare}>
        <p>
            People often compare AI by comparing brains. But what you actually use is a combination: the brain, the agent
            harness around it, the tools it carries, and plenty more, like its instructions, its settings and how you
            phrase your request.
        </p>
        <p>
            Not every pairing is even possible: {TIED.name} only works with {TIED.onlyFrom}'s brains. Swap any one piece
            and the result changes. A great brain in clumsy arms can do worse than a smaller brain in good ones. That's
            why "which AI is best?" rarely has a simple answer, and why the answer keeps changing.
        </p>
    </Page>
</Act>

<Act label="Act 4 · Now">
    <Page title="One problem: brains make mistakes" scene={act4.mistakeOnRealPc}>
        <p>
            A model can misunderstand you, or be confidently wrong. When all it could do was talk, a wrong answer was just
            a wrong answer.
        </p>
        <p>
            With arms, a wrong answer becomes a <Term>wrong action</Term>. On your own computer, that could mean your
            photos.
        </p>
    </Page>

    <Page title="So the agent gets its own computer" scene={act4.sandboxAppears}>
        <p>
            A <Term>sandbox</Term> is a pretend computer that lives inside your real one. It behaves like a normal computer
            (it runs an operating system called Linux), but it is walled off from your stuff.
        </p>
        <p>
            Inside, the agent can do whatever it wants. If it makes a mess, the mess stays in the sandbox, and you can
            throw the sandbox away and start fresh.
        </p>
    </Page>

    <Page title="Sharing one folder with the sandbox" scene={act4.sharingOneFolder}>
        <p>
            A sandbox starts out empty, but the agent usually needs to work on something of yours, like a website you are
            building. So you <Term>share a folder</Term> with the sandbox.
        </p>
        <p>
            The shared folder shows up in both places, in the same colour: it is one folder, seen from outside and from
            inside. What the agent changes there, you see too. Everything you did not share stays out of reach, so share
            only what you are happy for the agent to work on.
        </p>
    </Page>
</Act>

<Act label="Act 5 · Beyond the sandbox">
    <Page title="One layer, not the whole answer" scene={act5.layersOfDefence}>
        <p>
            The sandbox is <Term>one approach</Term>, not the whole answer. Keeping everyone safe with agents will always
            be a journey: many approaches layered together, with new ones added as people find new ways to misuse them.
        </p>
        <p>
            Email went the same way. It was built to send letters, then people used it for spam, phishing (fake messages
            that trick you into handing over passwords) and fraud. Today spam filters, warning labels and sender checks
            each catch some of it. None of them is enough alone; together they help.
        </p>
        <p>
            This app is one of those layers, there to help you out. Your own judgement is still part of the picture, as
            the next page shows.
        </p>
    </Page>

    <Page title="One way out: through you" scene={act5.outThroughYou}>
        <p>
            A sandbox only protects you from what stays inside it. Anything that comes out, through the shared folder or in
            a chat message, is up to you.
        </p>
        <p>
            Here the agent saves a program called <Term>click-me.exe</Term> in the shared folder and asks you to open it.
            Opening it on your own computer runs it outside the sandbox, with the same reach as you. Agents can be tricked
            by something they read, or simply be wrong, so treat what they hand you like a message from a stranger: only
            open a program if you know what it does.
        </p>
    </Page>
</Act>

<Act label="Recap">
    <Page title="The whole picture" scene={act5.wholePicture}>
        <Glossary>
            <GlossaryItem term="Model">
                The brain. It thinks and writes text. {andList(MAKERS.slice(0, 3).map((m) => m.models[0]))} are models.
            </GlossaryItem>
            <GlossaryItem term="Agent">
                The arms. It does what the brain decides. Also called an agent harness, like
                {andList(HARNESSES.slice(0, 3).map((h) => h.name))}.
            </GlossaryItem>
            <GlossaryItem term="Tool">
                Something the arms pick up to do one job, like reading a file or searching the web.
            </GlossaryItem>
            <GlossaryItem term="Chat">Where you tell them what you want.</GlossaryItem>
            <GlossaryItem term="Terminal">How the arms operate a computer: typed commands.</GlossaryItem>
            <GlossaryItem term="Sandbox">A safe, separate computer inside yours for the agent to work in.</GlossaryItem>
            <GlossaryItem term="Shared folder">
                A folder on your computer that the sandbox can see too. Same folder, same colour, in both places.
            </GlossaryItem>
        </Glossary>
    </Page>
</Act>
