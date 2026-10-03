// gll2wavey converts an EASE GLL loudspeaker file into a Wavey .waveyspk
// model, using the GLL parser and array engine from
// https://github.com/CWBudde/gll-tools (MIT). It is built inside a
// gll-tools checkout because it uses that module's internal filter code;
// see scripts/gll2wavey.sh.
//
// All sources of the box are combined coherently in the far field, each with
// its own position, balloon, on-axis spectrum and filter group, then
// normalized to 1 m with the propagation delay to the reference point
// removed. The result is a single complex balloon in Wavey's convention.
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"math"
	"math/cmplx"
	"os"
	"strings"

	"github.com/cwbudde/gll-tools/internal/filters"
	"github.com/cwbudde/gll-tools/pkg/gll"
)

type reference struct {
	DistanceM       float64 `json:"distance_m"`
	InputVoltageRMS float64 `json:"input_voltage_rms"`
}

type onAxis struct {
	MagnitudeDBSPL []float64 `json:"magnitude_db_spl"`
	PhaseDeg       []float64 `json:"phase_deg"`
}

type enclosure struct {
	WidthM  float64 `json:"width_m"`
	HeightM float64 `json:"height_m"`
	DepthM  float64 `json:"depth_m"`
}

// geometry is the cabinet wireframe in Wavey's local frame (x forward,
// y left, z up), relative to the enclosure centre, in metres.
type geometry struct {
	Vertices [][3]float64 `json:"vertices"`
	Edges    [][2]uint32  `json:"edges"`
}

// rigging: how boxes hang together (line arrays), in the same local frame
// as the geometry. Splays are degrees; a positive splay tilts the lower
// box further down.
type rigPoint struct {
	Label    string     `json:"label"`
	Position [3]float64 `json:"position"`
}

type rigFrame struct {
	Key          string      `json:"key"`
	Label        string      `json:"label"`
	Flown        bool        `json:"flown"`
	WeightKg     float64     `json:"weight_kg"`
	CenterOfMass *[3]float64 `json:"center_of_mass,omitempty"`
	// Flown: where the top box's pivot attaches. Ground stack: where the
	// bottom box's next pivot rests.
	Attach     [3]float64 `json:"attach"`
	PickPoints []rigPoint `json:"pick_points,omitempty"`
	SplaysDeg  []float64  `json:"splays_deg"`
	MaxBoxes   *int       `json:"max_boxes,omitempty"`
	Geometry   *geometry  `json:"geometry,omitempty"`
}

type rigPreset struct {
	Label     string    `json:"label"`
	SplaysDeg []float64 `json:"splays_deg"`
}

type rigging struct {
	Pivot        [3]float64  `json:"pivot"`
	NextPivot    [3]float64  `json:"next_pivot"`
	CenterOfMass *[3]float64 `json:"center_of_mass,omitempty"`
	WeightKg     float64     `json:"weight_kg"`
	SplaysDeg    []float64   `json:"splays_deg"`
	Frames       []rigFrame  `json:"frames,omitempty"`
	Presets      []rigPreset `json:"presets,omitempty"`
}

type balloon struct {
	ThetaDeg    []float64 `json:"theta_deg"`
	PhiDeg      []float64 `json:"phi_deg"`
	MagnitudeDB []float32 `json:"magnitude_db"`
	PhaseDeg    []float32 `json:"phase_deg"`
}

type speaker struct {
	Format          string     `json:"format"`
	Version         int        `json:"version"`
	ID              string     `json:"id"`
	Name            string     `json:"name"`
	Manufacturer    string     `json:"manufacturer,omitempty"`
	FrequenciesHz   []float64  `json:"frequencies_hz"`
	Reference       reference  `json:"reference"`
	OnAxis          onAxis     `json:"on_axis"`
	MaxInputVoltage *float64   `json:"max_input_voltage_rms,omitempty"`
	LatencyS        float64    `json:"latency_s"`
	AcousticCenterM [3]float64 `json:"acoustic_center_m"`
	Enclosure       enclosure  `json:"enclosure"`
	Balloon         *balloon   `json:"balloon,omitempty"`
	Geometry        *geometry  `json:"geometry,omitempty"`
	Rigging         *rigging   `json:"rigging,omitempty"`
	NominalBandHz   *[2]float64 `json:"nominal_band_hz,omitempty"`
	PhaseEstimated  bool       `json:"phase_estimated"`
}

// GLL box coordinates: x sideways, y depth (front face at y = 0, +y toward
// the back), z up — the cabinet wireframes and driver positions both follow
// this. The gll-tools array engine uses x forward, y right, z up.
func boxToEngine(v gll.Vector3D) gll.Vector3D {
	return gll.Vector3D{X: -v.Y, Y: v.X, Z: v.Z}
}

// Engine frame (x fwd, y right, z up) to Wavey's local frame (x fwd, y left, z up).
func engineToWavey(v gll.Vector3D) [3]float64 {
	return [3]float64{v.X, -v.Y, v.Z}
}

func fail(format string, a ...any) {
	fmt.Fprintf(os.Stderr, "gll2wavey: "+format+"\n", a...)
	os.Exit(1)
}

func slug(s string) string {
	var b strings.Builder
	dash := false
	for _, r := range strings.ToLower(s) {
		if (r >= 'a' && r <= 'z') || (r >= '0' && r <= '9') {
			b.WriteRune(r)
			dash = false
		} else if !dash && b.Len() > 0 {
			b.WriteByte('-')
			dash = true
		}
	}
	return strings.Trim(b.String(), "-")
}

func findGroup(f *gll.File, key string) int {
	for i, g := range f.Database.FilterGroups {
		if g.Key == key {
			return i
		}
	}
	return -1
}

// firAt evaluates a frequency-domain FIR (complex or magnitude/phase data on
// a linear grid from 0 to Nyquist) at f by linear interpolation.
func firAt(d *gll.FIRFilterData, f float64) (complex128, bool) {
	n := len(d.DataIRM)
	if d.IsTimeResponse || d.SampleRate <= 0 || n < 2 {
		return 0, false
	}
	val := func(i int) complex128 {
		if d.IsComplex {
			return complex(d.DataIRM[i], d.DataDIP[i])
		}
		ph := 0.0
		if len(d.DataDIP) == n {
			ph = d.DataDIP[i]
		}
		return cmplx.Rect(d.DataIRM[i], ph)
	}
	if d.IsComplex && len(d.DataDIP) != n {
		return 0, false
	}
	x := f / (d.SampleRate / 2) * float64(n-1)
	if x >= float64(n-1) {
		return val(n - 1), true
	}
	i := int(x)
	t := x - float64(i)
	return val(i)*complex(1-t, 0) + val(i+1)*complex(t, 0), true
}

// filterSpectrum is the response of one filter definition (a chain of
// filters with its own gain, delay and polarity) on the balloon's grid.
// Frequency-domain FIRs are evaluated directly at each frequency (several
// FIRs at different sample rates are all applied); other kinds go through
// gll-tools' filter engine.
func filterSpectrum(f *gll.File, def gll.LogSpectrumDefinition, group, index int) *gll.TransferFunction {
	fd := f.Database.FilterGroups[group].Filters[index]
	bank := fd.Filter
	if bank == nil {
		return nil
	}
	n := int(def.PointCount)
	h := make([]complex128, n)
	for i := range h {
		h[i] = 1
	}
	scale := func(gainDB, delay float64, invert bool) {
		g := math.Pow(10, gainDB/20)
		if invert {
			g = -g
		}
		for i := range h {
			h[i] *= cmplx.Rect(g, -2*math.Pi*def.GetFrequency(i)*delay)
		}
	}
	allFIR := len(bank.Filters) > 0
	for _, b := range bank.Filters {
		if b.FIRData == nil || b.FIRData.IsTimeResponse {
			allFIR = false
		}
	}
	if !allFIR {
		r := filters.BuildFilterResponse(f, filters.FilterResponseRequest{GroupIndex: group, FilterIndex: index})
		if !r.Success || len(r.Level) == 0 {
			fmt.Fprintf(os.Stderr, "warning: filter %q of group %q unusable: %s %s\n", fd.Label, f.Database.FilterGroups[group].Key, r.Error, r.Message)
			return nil
		}
		// Interpolate (log frequency) onto the balloon grid.
		for i := range h {
			fr := def.GetFrequency(i)
			k := 0
			for k+1 < len(r.Frequencies) && r.Frequencies[k+1] < fr {
				k++
			}
			lv, ph := r.Level[k], 0.0
			if len(r.Phase) == len(r.Level) {
				ph = r.Phase[k]
			}
			h[i] = cmplx.Rect(math.Pow(10, lv/20), ph)
		}
	} else {
		if bank.ByPass {
			return nil
		}
		for _, b := range bank.Filters {
			if b.ByPass {
				continue
			}
			for i := range h {
				v, _ := firAt(b.FIRData, def.GetFrequency(i))
				h[i] *= v
			}
			scale(b.Gain, b.Delay, b.InvertPolarity)
		}
		scale(bank.Gain, bank.Delay, bank.InvertPolarity)
	}
	tf := &gll.TransferFunction{Definition: def, Level: make([]float64, n), Phase: make([]float64, n)}
	for i, v := range h {
		tf.Level[i] = 20 * math.Log10(math.Max(cmplx.Abs(v), 1e-9))
		tf.Phase[i] = cmplx.Phase(v)
	}
	return tf
}

// boxPoint converts a GLL box/frame point to Wavey's local axes.
func boxPoint(v gll.Vector3D) [3]float64 {
	return engineToWavey(boxToEngine(v))
}

// caseVertices returns a case geometry's vertex pairs (one pair per edge),
// with the mirrored half added for symmetric geometries (stored as one half,
// mirrored about the plane x = SymmetryAxis).
func caseVertices(g *gll.CaseGeometry) []gll.Vector3D {
	var out []gll.Vector3D
	for _, v := range g.Vertices {
		out = append(out, gll.Vector3D{X: v.X, Y: v.Y, Z: v.Z})
	}
	if g.IsSymmetric {
		n := len(out) / 2 * 2
		for i := 0; i < n; i++ {
			v := out[i]
			out = append(out, gll.Vector3D{X: 2*g.SymmetryAxis - v.X, Y: v.Y, Z: v.Z})
		}
	}
	return out
}

// wireframe converts a GLL case geometry (vertex pairs, one per edge).
func wireframe(g *gll.CaseGeometry, center [3]float64) *geometry {
	if g == nil || len(g.Vertices) < 2 {
		return nil
	}
	geo := &geometry{}
	verts := caseVertices(g)
	for _, v := range verts {
		p := boxPoint(v)
		geo.Vertices = append(geo.Vertices, [3]float64{p[0] - center[0], p[1] - center[1], p[2] - center[2]})
	}
	for i := 0; i+1 < len(verts); i += 2 {
		geo.Edges = append(geo.Edges, [2]uint32{uint32(i), uint32(i + 1)})
	}
	return geo
}

func degrees(a []gll.LabeledValueD) []float64 {
	var out []float64
	for _, x := range a {
		out = append(out, math.Round(x.Value*180/math.Pi*1000)/1000)
	}
	return out
}

// buildRigging collects the box's pivots, the box-to-box splays and the
// frames that can carry it. nil when the file has no line-array rigging.
func buildRigging(db *gll.Database, box gll.BoxType, center [3]float64) *rigging {
	if box.NextPivot == nil {
		return nil
	}
	rel := func(v gll.Vector3D) [3]float64 {
		p := boxPoint(v)
		return [3]float64{p[0] - center[0], p[1] - center[1], p[2] - center[2]}
	}
	r := &rigging{Pivot: rel(gll.Vector3D{}), NextPivot: rel(*box.NextPivot), WeightKg: box.Weight}
	if box.CenterOfMass != nil {
		c := rel(*box.CenterOfMass)
		r.CenterOfMass = &c
	}
	for _, c := range db.Connectors {
		if c.UpperBox == box.Key && c.LowerBox == box.Key {
			r.SplaysDeg = degrees(c.Angles)
		}
	}
	if r.SplaysDeg == nil {
		return nil
	}
	maxCount := map[string]int{}
	for _, l := range db.Limits {
		if l.Type == gll.LimitTypeMaxCount && l.Frame != "" {
			maxCount[l.Frame] = int(l.LimitValue)
		}
	}
	for _, fr := range db.Frames {
		f := rigFrame{Key: fr.Key, Label: fr.Label, Flown: fr.TypeFlown, WeightKg: fr.Weight}
		if fr.NextPivot != nil {
			f.Attach = boxPoint(*fr.NextPivot)
		}
		if fr.CenterOfMass != nil {
			c := boxPoint(*fr.CenterOfMass)
			f.CenterOfMass = &c
		}
		for _, p := range fr.PinPoints {
			f.PickPoints = append(f.PickPoints, rigPoint{Label: p.Label, Position: boxPoint(p.Vector)})
		}
		for _, c := range db.Connectors {
			if (fr.TypeFlown && c.UpperBox == fr.Key && c.LowerBox == box.Key) || (!fr.TypeFlown && c.UpperBox == box.Key && c.LowerBox == fr.Key) {
				f.SplaysDeg = degrees(c.Angles)
			}
		}
		if f.SplaysDeg == nil {
			continue // this frame cannot carry the box
		}
		if n, ok := maxCount[fr.Key]; ok {
			f.MaxBoxes = &n
		}
		f.Geometry = wireframe(fr.CaseGeometry, [3]float64{})
		r.Frames = append(r.Frames, f)
	}
	// Preset arrays of this box only: splays from successive box angles.
	for _, cs := range db.ClusterSetups {
		var splays []float64
		ok := len(cs.Setup.Boxes) > 1
		for i, b := range cs.Setup.Boxes {
			if b.BoxTypeKey != box.Key {
				ok = false
				break
			}
			if i > 0 {
				d := (b.Angles.Y - cs.Setup.Boxes[i-1].Angles.Y) * 180 / math.Pi
				splays = append(splays, math.Round(math.Abs(d)*100)/100)
			}
		}
		if ok {
			r.Presets = append(r.Presets, rigPreset{Label: cs.Label, SplaysDeg: splays})
		}
	}
	return r
}

func main() {
	in := flag.String("in", "", "input .gll file")
	out := flag.String("out", "", "output .waveyspk file")
	id := flag.String("id", "", "model id (default: from the GLL label)")
	name := flag.String("name", "", "model name (default: manufacturer + label)")
	maker := flag.String("manufacturer", "", "manufacturer (default: from the GLL file)")
	filterKeys := flag.String("filters", "", "comma-separated filter group keys, one per box source in order (empty: none)")
	preset := flag.Int("preset", 0, "which filter definition of each group to use (groups can hold alternatives, e.g. with and without a sub)")
	list := flag.Bool("list", false, "list box sources and filter groups, then exit")
	fmin := flag.Float64("fmin", 20, "lowest frequency to export (Hz)")
	fmax := flag.Float64("fmax", 20500, "highest frequency to export (Hz)")
	only := flag.Int("only", -1, "use only this source index (for inspection)")
	size := flag.String("size", "", "enclosure w,h,d in metres when the file has no case geometry")
	rollFlag := flag.String("roll", "auto", "extra roll of every source balloon about its axis in degrees (90 swaps the H and V planes), or \"auto\" to pick 0 or 90 by the file's nominal coverage angles")
	originFlag := flag.String("origin", "reference", "balloon origin and acoustic centre: \"reference\" (the file's reference point, if any) or \"sources\" (centroid of the source positions; use when the reference point is a rigging point away from the drivers)")
	flag.Parse()
	if *in == "" {
		fail("-in is required")
	}

	data, err := os.ReadFile(*in)
	if err != nil {
		fail("%v", err)
	}
	f, err := gll.Parse(bytes.NewReader(data))
	if err != nil {
		fail("parsing: %v", err)
	}
	db := f.Database
	if db == nil || len(db.BoxTypes) == 0 {
		fail("no box types in file")
	}
	box := db.BoxTypes[0]
	srcDef := func(key string) *gll.SourceDefinition {
		for _, s := range db.SourceDefinitions {
			if s.Key == key {
				return s.Definition
			}
		}
		return nil
	}

	if *list {
		fmt.Printf("box %q (%d sources), nominal %g° H × %g° V\n", box.Label, len(box.SourcePlacements), box.HorizontalOpeningAngle, box.VerticalOpeningAngle)
		for i, p := range box.SourcePlacements {
			fmt.Printf("  source %d: %q def=%s pos=(%g, %g, %g) angles=(%g, %g, %g)\n", i, p.Label, p.SourceDefKey,
				p.Position.X, p.Position.Y, p.Position.Z, p.Angles.X, p.Angles.Y, p.Angles.Z)
		}
		for _, g := range db.FilterGroups {
			var defs []string
			for i, fd := range g.Filters {
				defs = append(defs, fmt.Sprintf("%d=%q", i, fd.Label))
			}
			fmt.Printf("  filter group %s: %q, definitions %s\n", g.Key, g.Label, strings.Join(defs, " "))
		}
		v := func(p *gll.Vector3D) string {
			if p == nil {
				return "—"
			}
			return fmt.Sprintf("(%.4g, %.4g, %.4g)", p.X, p.Y, p.Z)
		}
		for _, bt := range db.BoxTypes {
			fmt.Printf("  box type %s %q: reference %s, next pivot %s, %.1f kg\n", bt.Key, bt.Label, v(bt.ReferencePoint), v(bt.NextPivot), bt.Weight)
		}
		for _, fr := range db.Frames {
			fmt.Printf("  frame %s %q: flown %v, %.1f kg, next pivot %s, %d pin points\n", fr.Key, fr.Label, fr.TypeFlown, fr.Weight, v(fr.NextPivot), len(fr.PinPoints))
			for _, p := range fr.PinPoints {
				fmt.Printf("    pin %q %s\n", p.Label, v(&p.Vector))
			}
		}
		for _, c := range db.Connectors {
			var a []string
			for _, x := range c.Angles {
				a = append(a, fmt.Sprintf("%s=%g", x.Label, x.Value))
			}
			fmt.Printf("  connector %s → %s (frame %q): %s\n", c.UpperBox, c.LowerBox, c.Frame, strings.Join(a, " "))
		}
		for _, cs := range db.ClusterSetups {
			fmt.Printf("  cluster setup %s %q: %d boxes\n", cs.Key, cs.Label, len(cs.Setup.Boxes))
			for _, b := range cs.Setup.Boxes {
				fmt.Printf("    %s pos %s angles %s\n", b.BoxTypeKey, v(&b.Position), v(&b.Angles))
			}
		}
		for _, l := range db.Limits {
			fmt.Printf("  limit %+v\n", l)
		}
		return
	}

	var keys []string
	if *filterKeys != "" {
		keys = strings.Split(*filterKeys, ",")
		if len(keys) != len(box.SourcePlacements) {
			fail("-filters needs %d keys (one per source), got %d", len(box.SourcePlacements), len(keys))
		}
	}

	// One array element per source placement, as in the gll-tools demo.
	var def gll.LogSpectrumDefinition
	var first *gll.SourceDefinition
	var origin gll.Vector3D
	axial := true
	type placed struct {
		el gll.ArrayElement
	}
	var placedEls []placed
	for i, p := range box.SourcePlacements {
		if *only >= 0 && i != *only {
			continue
		}
		sd := srcDef(p.SourceDefKey)
		if sd == nil || sd.BalloonData == nil {
			fail("source %q has no balloon data", p.Label)
		}
		if len(sd.BalloonData.Responses) == 0 {
			if err := gll.LoadBalloonResponses(bytes.NewReader(data), sd.BalloonData); err != nil {
				fail("loading balloon of %q: %v", p.Label, err)
			}
		}
		if first == nil {
			first = sd
			def = sd.BalloonData.Responses[0].Definition
		}
		if sd.BalloonData.AngularResolution.MeridianStep > 0 && len(sd.BalloonData.Responses) > int(180/sd.BalloonData.AngularResolution.ParallelStep)+1 {
			axial = false
		}
		el := gll.ArrayElement{Position: boxToEngine(p.Position), Angles: p.Angles, SourceDefs: []*gll.SourceDefinition{sd}}
		if keys != nil && keys[i] != "" {
			g := findGroup(f, strings.TrimSpace(keys[i]))
			if g < 0 {
				fail("unknown filter group %q", keys[i])
			}
			if *preset >= len(f.Database.FilterGroups[g].Filters) {
				fail("filter group %q has no definition %d", keys[i], *preset)
			}
			if tf := filterSpectrum(f, def, g, *preset); tf != nil {
				el.FilterSpectra = []*gll.TransferFunction{tf}
			}
		}
		placedEls = append(placedEls, placed{el})
		ep := boxToEngine(p.Position)
		origin.X += ep.X
		origin.Y += ep.Y
		origin.Z += ep.Z
	}
	n := float64(len(placedEls))
	origin = gll.Vector3D{X: origin.X / n, Y: origin.Y / n, Z: origin.Z / n}
	if box.ReferencePoint != nil && *only < 0 && *originFlag != "sources" {
		ref := boxToEngine(*box.ReferencePoint)
		// A reference point outside the cabinet is a rigging or stand point,
		// not where the sound comes from: keep the drivers' centroid then.
		inside := true
		if g := box.CaseGeometry; g != nil && len(g.Vertices) >= 2 {
			lo := [3]float64{math.Inf(1), math.Inf(1), math.Inf(1)}
			hi := [3]float64{math.Inf(-1), math.Inf(-1), math.Inf(-1)}
			for _, v := range caseVertices(g) {
				e := boxToEngine(v)
				for k, c := range [3]float64{e.X, e.Y, e.Z} {
					lo[k] = math.Min(lo[k], c)
					hi[k] = math.Max(hi[k], c)
				}
			}
			const margin = 0.05
			for k, c := range [3]float64{ref.X, ref.Y, ref.Z} {
				if c < lo[k]-margin || c > hi[k]+margin {
					inside = false
				}
			}
		}
		if inside {
			origin = ref
		} else {
			fmt.Fprintf(os.Stderr, "note: the reference point (%.2f, %.2f, %.2f) lies outside the cabinet; using the drivers' centroid as the acoustic centre\n", ref.X, ref.Y, ref.Z)
		}
	}

	makeConfig := func(rollDeg float64) *gll.ArrayConfig {
		cfg := &gll.ArrayConfig{}
		for _, p := range placedEls {
			el := p.el
			el.Angles.X += rollDeg * math.Pi / 180 // rotation about the firing axis (x)
			cfg.Elements = append(cfg.Elements, el)
		}
		return cfg
	}
	cfg := makeConfig(0)

	air := gll.DefaultAirProperties()
	const R = 200.0
	// Far-field response in direction (x fwd, y right, z up) of the GLL frame,
	// scaled to 1 m with the propagation delay from the origin removed.
	response := func(dx, dy, dz float64) []complex128 {
		rx := gll.Vector3D{X: origin.X + R*dx, Y: origin.Y + R*dy, Z: origin.Z + R*dz}
		tf := gll.ComputeSystemResponseAt(cfg, rx, air, false)
		if tf == nil {
			fail("no response")
		}
		outv := make([]complex128, len(tf.Level))
		for i := range tf.Level {
			fr := tf.Definition.GetFrequency(i)
			ph := tf.Phase[i] + 2*math.Pi*fr*R/air.Speed
			// Levels are dB SPL (re 20 µPa); convert to pascals at 1 m.
			outv[i] = cmplx.Rect(20e-6*math.Pow(10, tf.Level[i]/20)*R, ph)
		}
		return outv
	}

	var fidx []int
	var freqs []float64
	for i := 0; i < int(def.PointCount); i++ {
		fr := def.GetFrequency(i)
		if fr >= *fmin && fr <= *fmax {
			fidx = append(fidx, i)
			freqs = append(freqs, fr)
		}
	}

	// Balloon orientation. gll-tools documents meridian 0° as up; for some
	// files the nominal coverage angles only match with the planes swapped,
	// so "auto" picks whichever orientation agrees with them at 2–10 kHz.
	roll := 0.0
	switch {
	case *rollFlag == "auto":
		if !axial && box.HorizontalOpeningAngle > 0 && box.VerticalOpeningAngle > 0 && box.HorizontalOpeningAngle != box.VerticalOpeningAngle {
			coverage := func(rollDeg float64) (h, v float64) {
				cfg = makeConfig(rollDeg)
				var cnt float64
				for _, fr := range []float64{2000, 3150, 5000, 8000, 10000} {
					k := 0
					for i := range fidx {
						if math.Abs(math.Log(def.GetFrequency(fidx[i])/fr)) < math.Abs(math.Log(def.GetFrequency(fidx[k])/fr)) {
							k = i
						}
					}
					fi := fidx[k]
					on := response(1, 0, 0)[fi]
					edge := func(dir func(t float64) (float64, float64, float64)) float64 {
						for t := 5.0; t <= 180; t += 5 {
							x, y, z := dir(t * math.Pi / 180)
							if cmplx.Abs(response(x, y, z)[fi]/on) < 0.5 {
								return 2 * t
							}
						}
						return 360
					}
					h += edge(func(t float64) (float64, float64, float64) { return math.Cos(t), math.Sin(t), 0 })
					v += edge(func(t float64) (float64, float64, float64) { return math.Cos(t), 0, math.Sin(t) })
					cnt++
				}
				return h / cnt, v / cnt
			}
			h0, v0 := coverage(0)
			h9, v9 := coverage(90)
			nh, nv := box.HorizontalOpeningAngle, box.VerticalOpeningAngle
			err0 := math.Abs(h0-nh) + math.Abs(v0-nv)
			err9 := math.Abs(h9-nh) + math.Abs(v9-nv)
			if err9 < err0 {
				roll = 90
			}
			fmt.Printf("  orientation: nominal %g° H × %g° V; as stored %.0f° × %.0f°, rotated 90° %.0f° × %.0f° → using roll %g°\n",
				nh, nv, h0, v0, h9, v9, roll)
		}
	default:
		if _, err := fmt.Sscanf(*rollFlag, "%g", &roll); err != nil {
			fail("-roll must be a number or auto")
		}
	}
	cfg = makeConfig(roll)

	on := response(1, 0, 0)
	m := speaker{Format: "waveyspk", Version: 1, FrequenciesHz: freqs}
	for _, i := range fidx {
		m.OnAxis.MagnitudeDBSPL = append(m.OnAxis.MagnitudeDBSPL, 20*math.Log10(math.Max(cmplx.Abs(on[i]), 1e-12)/20e-6))
		m.OnAxis.PhaseDeg = append(m.OnAxis.PhaseDeg, cmplx.Phase(on[i])*180/math.Pi)
	}

	// Wavey balloon: θ from +x, φ from +y_left toward +z. GLL y points right.
	b := &balloon{}
	for t := 0; t <= 180; t += 5 {
		b.ThetaDeg = append(b.ThetaDeg, float64(t))
	}
	for p := 0; p < 360; p += 5 {
		b.PhiDeg = append(b.PhiDeg, float64(p))
	}
	nt, np, nf := len(b.ThetaDeg), len(b.PhiDeg), len(fidx)
	b.MagnitudeDB = make([]float32, nf*nt*np)
	b.PhaseDeg = make([]float32, nf*nt*np)
	for ti, t := range b.ThetaDeg {
		for pi, p := range b.PhiDeg {
			th, ph := t*math.Pi/180, p*math.Pi/180
			lx, ly, lz := math.Cos(th), math.Sin(th)*math.Cos(ph), math.Sin(th)*math.Sin(ph)
			r := response(lx, -ly, lz)
			for k, i := range fidx {
				d := r[i] / on[i]
				if cmplx.Abs(on[i]) < 1e-12 {
					d = 1
				}
				j := (k*nt+ti)*np + pi
				b.MagnitudeDB[j] = float32(20 * math.Log10(math.Max(cmplx.Abs(d), 1e-6)))
				b.PhaseDeg[j] = float32(cmplx.Phase(d) * 180 / math.Pi)
			}
		}
	}
	m.Balloon = b

	label := box.Label
	manufacturer := strings.TrimSpace(f.GenSystem.Company)
	if manufacturer == "" {
		manufacturer = strings.TrimSpace(first.CompanyLabel)
	}
	if *maker != "" {
		manufacturer = *maker
	}
	m.Manufacturer = manufacturer
	m.Name = strings.TrimSpace(manufacturer + " " + label)
	if *name != "" {
		m.Name = *name
	}
	m.ID = slug(m.Name)
	if *id != "" {
		m.ID = *id
	}
	// Nominal band: the widest range over all sources.
	lo, hi := math.Inf(1), 0.0
	for _, e := range cfg.Elements {
		sd := e.SourceDefs[0]
		if sd.NominalBandwidthFrom > 0 && sd.NominalBandwidthTo > sd.NominalBandwidthFrom {
			lo, hi = math.Min(lo, sd.NominalBandwidthFrom), math.Max(hi, sd.NominalBandwidthTo)
		}
	}
	if hi > 0 {
		m.NominalBandHz = &[2]float64{lo, hi}
	}
	m.Reference = reference{DistanceM: 1, InputVoltageRMS: first.MeasuredVoltage}
	if m.Reference.InputVoltageRMS <= 0 {
		m.Reference.InputVoltageRMS = 2.828
	}
	if first.MaxVoltage > 0 {
		v := first.MaxVoltage
		m.MaxInputVoltage = &v
	}

	// Enclosure from the case geometry: a wireframe whose vertices come in
	// pairs, one pair per edge, in box coordinates (metres).
	m.Enclosure = enclosure{WidthM: 0.5, HeightM: 0.6, DepthM: 0.5}
	center := engineToWavey(origin)
	if g := box.CaseGeometry; g != nil && len(g.Vertices) >= 2 {
		var pts [][3]float64
		for _, v := range caseVertices(g) {
			pts = append(pts, boxPoint(v))
		}
		lo := [3]float64{math.Inf(1), math.Inf(1), math.Inf(1)}
		hi := [3]float64{math.Inf(-1), math.Inf(-1), math.Inf(-1)}
		for _, p := range pts {
			for k := 0; k < 3; k++ {
				lo[k] = math.Min(lo[k], p[k])
				hi[k] = math.Max(hi[k], p[k])
			}
		}
		if hi[0]-lo[0] > 0.02 && hi[1]-lo[1] > 0.02 && hi[2]-lo[2] > 0.02 {
			m.Enclosure = enclosure{DepthM: hi[0] - lo[0], WidthM: hi[1] - lo[1], HeightM: hi[2] - lo[2]}
			center = [3]float64{(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2}
			m.Geometry = wireframe(g, center)
		}
	}
	if *size != "" && m.Geometry == nil {
		var w, h, d float64
		if _, err := fmt.Sscanf(*size, "%g,%g,%g", &w, &h, &d); err != nil {
			fail("-size must be w,h,d")
		}
		m.Enclosure = enclosure{WidthM: w, HeightM: h, DepthM: d}
	}
	m.Rigging = buildRigging(db, box, center)
	o := engineToWavey(origin)
	m.AcousticCenterM = [3]float64{o[0] - center[0], o[1] - center[1], o[2] - center[2]}

	// Report.
	at := func(fr float64) int {
		best := 0
		for k := range freqs {
			if math.Abs(math.Log(freqs[k]/fr)) < math.Abs(math.Log(freqs[best]/fr)) {
				best = k
			}
		}
		return best
	}
	fmt.Printf("%s: %d frequencies %.0f–%.0f Hz, %d sources, filters: %v\n", m.Name, nf, freqs[0], freqs[nf-1], len(cfg.Elements), keys)
	geomNote := "no geometry"
	if m.Geometry != nil {
		geomNote = fmt.Sprintf("%d-edge wireframe", len(m.Geometry.Edges))
	}
	fmt.Printf("  enclosure %.2f × %.2f × %.2f m (w × h × d, %s), acoustic centre offset (%.2f, %.2f, %.2f) m\n",
		m.Enclosure.WidthM, m.Enclosure.HeightM, m.Enclosure.DepthM, geomNote, m.AcousticCenterM[0], m.AcousticCenterM[1], m.AcousticCenterM[2])
	if r := m.Rigging; r != nil {
		fmt.Printf("  rigging: pivot (%.3f, %.3f, %.3f), next pivot (%.3f, %.3f, %.3f), splays %v°\n",
			r.Pivot[0], r.Pivot[1], r.Pivot[2], r.NextPivot[0], r.NextPivot[1], r.NextPivot[2], r.SplaysDeg)
		for _, f := range r.Frames {
			fmt.Printf("    frame %q (flown %v): attach (%.3f, %.3f, %.3f), %d pick points, splays %v°\n", f.Label, f.Flown, f.Attach[0], f.Attach[1], f.Attach[2], len(f.PickPoints), f.SplaysDeg)
		}
	}
	fmt.Print("  on-axis dB SPL @1m/", m.Reference.InputVoltageRMS, "V:")
	for _, fr := range []float64{31.5, 63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000} {
		if fr >= freqs[0] && fr <= freqs[nf-1] {
			fmt.Printf(" %g:%.1f", fr, m.OnAxis.MagnitudeDBSPL[at(fr)])
		}
	}
	fmt.Println()
	// −6 dB coverage (full angle) horizontally and vertically.
	for _, fr := range []float64{500, 1000, 1600, 2500, 4000, 6300, 10000} {
		if fr < freqs[0] || fr > freqs[nf-1] {
			continue
		}
		k := at(fr)
		cov := func(phiIdx int) float64 {
			for ti := range b.ThetaDeg {
				if b.MagnitudeDB[(k*nt+ti)*np+phiIdx] < -6 {
					return 2 * b.ThetaDeg[ti]
				}
			}
			return 360
		}
		fmt.Printf("  −6 dB coverage at %g Hz: %g° H × %g° V\n", fr, cov(0), cov(np/4))
	}

	if *out != "" {
		js, err := json.Marshal(m)
		if err != nil {
			fail("%v", err)
		}
		if err := os.WriteFile(*out, js, 0o644); err != nil {
			fail("%v", err)
		}
		fmt.Println("  wrote", *out)
	}
}
