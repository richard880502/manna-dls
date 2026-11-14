"""Irregular inflections -> base form (hand-written, common items only; extend as needed)."""

_VERBS = """
be:am,is,are,was,were,been,being have:has,had,having do:does,did,done,doing go:goes,went,gone
say:said make:made get:got,gotten know:knew,known think:thought take:took,taken see:saw,seen
come:came give:gave,given find:found tell:told become:became leave:left feel:felt put:put
bring:brought begin:began,begun keep:kept hold:held write:wrote,written stand:stood hear:heard
let:let mean:meant set:set meet:met run:ran pay:paid sit:sat speak:spoke,spoken lie:lay,lain,lying
lead:led read:read grow:grew,grown lose:lost fall:fell,fallen send:sent build:built
understand:understood draw:drew,drawn break:broke,broken spend:spent cut:cut rise:rose,risen
drive:drove,driven buy:bought wear:wore,worn choose:chose,chosen seek:sought throw:threw,thrown
catch:caught deal:dealt win:won forget:forgot,forgotten lay:laid sell:sold fight:fought
bear:bore,born,borne teach:taught eat:ate,eaten sing:sang,sung strike:struck,stricken hang:hung
shake:shook,shaken ride:rode,ridden feed:fed shoot:shot drink:drank,drunk hit:hit arise:arose,arisen
fly:flew,flown blow:blew,blown bite:bit,bitten steal:stole,stolen swim:swam,swum sleep:slept
wake:woke,woken hide:hid,hidden freeze:froze,frozen forgive:forgave,forgiven shine:shone
sink:sank,sunk tear:tore,torn sweep:swept bend:bent bind:bound bleed:bled breed:bred burst:burst
cast:cast cling:clung creep:crept dig:dug dwell:dwelt flee:fled fling:flung forbid:forbade,forbidden
grind:ground kneel:knelt lend:lent light:lit ring:rang,rung shed:shed shrink:shrank,shrunk
shut:shut slide:slid spin:spun spit:spat split:split spread:spread spring:sprang,sprung
stick:stuck sting:stung stink:stank,stunk stride:strode swear:swore,sworn swing:swung
weave:wove,woven weep:wept wind:wound withdraw:withdrew,withdrawn overcome:overcame
undergo:underwent,undergone mistake:mistook,mistaken hurt:hurt cost:cost quit:quit
die:dying tie:tying show:shown sew:sewn sow:sown prove:proven swell:swollen can:could will:would
shall:should may:might learn:learnt burn:burnt dream:dreamt smell:smelt spell:spelt spill:spilt
"""
_NOUNS = """
man:men woman:women child:children person:people foot:feet tooth:teeth mouse:mice goose:geese
ox:oxen life:lives knife:knives wife:wives leaf:leaves half:halves shelf:shelves wolf:wolves
thief:thieves self:selves loaf:loaves calf:calves criterion:criteria phenomenon:phenomena
analysis:analyses crisis:crises basis:bases thesis:theses hypothesis:hypotheses
datum:data bacterium:bacteria curriculum:curricula fungus:fungi cactus:cacti stimulus:stimuli
"""
_ADJ = """
good:better,best bad:worse,worst far:farther,farthest,further,furthest little:less,least
many:more,most well:better,best
"""

IRREGULAR: dict[str, list[str]] = {}
for block in (_VERBS, _NOUNS, _ADJ):
    for item in block.split():
        base, forms = item.split(":")
        for f in forms.split(","):
            if base not in IRREGULAR.setdefault(f, []):
                IRREGULAR[f].append(base)

# function words / contractions fragments that never count as vocabulary items
CONTRACTION_PARTS = {"n't", "'s", "'re", "'ve", "'ll", "'d", "'m", "s", "t", "re", "ve", "ll", "d", "m"}
