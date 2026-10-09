import SwiftUI

struct Particle: Identifiable {
    let id = UUID()
    var x: CGFloat
    var y: CGFloat
    var scale: CGFloat
    var opacity: Double
    var color: Color
}

struct ParticleEffectView: View {
    @State private var particles: [Particle] = []
    let isTriggered: Bool
    
    var body: some View {
        Canvas { context, size in
            for particle in particles {
                let rect = CGRect(
                    x: particle.x - (10 * particle.scale) / 2,
                    y: particle.y - (10 * particle.scale) / 2,
                    width: 10 * particle.scale,
                    height: 10 * particle.scale
                )
                context.opacity = particle.opacity
                context.fill(Path(ellipseIn: rect), with: .color(particle.color))
            }
        }
        .onChange(of: isTriggered) { newValue in
            if newValue {
                fireParticles()
            }
        }
    }
    
    private func fireParticles() {
        var newParticles: [Particle] = []
        let colors: [Color] = [CRTheme.brandElectric, CRTheme.brandCyan, CRTheme.accentPink, CRTheme.accentPurple]
        
        for _ in 0..<30 {
            newParticles.append(Particle(
                x: 80, // Center of the portal approximately
                y: 100,
                scale: CGFloat.random(in: 0.5...1.5),
                opacity: 1.0,
                color: colors.randomElement()!
            ))
        }
        
        particles = newParticles
        
        withAnimation(.easeOut(duration: 0.8)) {
            for i in 0..<particles.count {
                let angle = Double.random(in: 0...(2 * .pi))
                let distance = CGFloat.random(in: 50...150)
                particles[i].x += CGFloat(cos(angle)) * distance
                particles[i].y += CGFloat(sin(angle)) * distance
                particles[i].scale = 0.1
                particles[i].opacity = 0
            }
        }
    }
}
