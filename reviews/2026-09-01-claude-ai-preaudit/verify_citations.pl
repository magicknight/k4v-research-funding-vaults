#!/usr/bin/env perl
use strict;
use warnings;
use File::Find qw(find);
use JSON::PP qw(encode_json);

my ($repo, $report) = @ARGV;
die "usage: verify_citations.pl REPO REPORT\n" unless defined $report;

open my $rfh, '<', $report or die "open report: $!";
my $text = do { local $/; <$rfh> };
close $rfh;

my @files;
find(
    {
        no_chdir => 1,
        wanted => sub {
            return if $File::Find::name =~ m{(?:^|/)\.git(?:/|$)};
            return unless -f $File::Find::name;
            my $rel = $File::Find::name;
            $rel =~ s{^\Q$repo\E/?}{};
            push @files, $rel;
        },
    },
    $repo,
);
@files = sort @files;

my %line_count;
sub lines_in {
    my ($rel) = @_;
    return $line_count{$rel} if exists $line_count{$rel};
    open my $fh, '<', "$repo/$rel" or return undef;
    my $n = 0;
    $n++ while <$fh>;
    close $fh;
    return $line_count{$rel} = $n;
}

my %seen;
my @citations;
while ($text =~ m{((?:[A-Za-z0-9_.-]+/)*(?:[A-Za-z0-9_.-]+)\.(?:rs|md|py|mjs|cjs|yml|yaml|toml|lock|json)):(\d[\d,-]*)}g) {
    my ($path, $spec) = ($1, $2);
    my $key = "$path:$spec";
    next if $seen{$key}++;
    push @citations, [$path, $spec];
}

my %bare_seen;
my @bare;
while ($text =~ /`(:\d[\d,-]*)`/g) {
    push @bare, $1 unless $bare_seen{$1}++;
}

my @results;
for my $citation (@citations) {
    my ($path, $spec) = @$citation;
    my @matches;
    my $resolution;
    if (-f "$repo/$path") {
        @matches = ($path);
        $resolution = 'EXACT';
    } elsif ($path =~ m{/\.\.\./}) {
        my $rx = quotemeta($path);
        $rx =~ s{\\/\\\.\\\.\\\.\\/}{/.*/};
        @matches = grep { /^.*$rx$/ } @files;
        $resolution = @matches == 1 ? 'UNIQUE_ELLIPSIS' : @matches > 1 ? 'AMBIGUOUS_ELLIPSIS' : 'MISSING';
    } else {
        @matches = grep { $_ eq $path || /(?:^|\/)\Q$path\E$/ } @files;
        if (!@matches && $path !~ m{/}) {
            @matches = grep { /(?:^|\/)\Q$path\E$/ } @files;
        }
        $resolution = @matches == 1 ? 'UNIQUE_SUFFIX' : @matches > 1 ? 'AMBIGUOUS_SUFFIX' : 'MISSING';
    }

    my @requested = ($spec =~ /(\d+)/g);
    my $max_requested = 0;
    for my $n (@requested) { $max_requested = $n if $n > $max_requested; }
    my @checked;
    my $all_in_range = @matches ? 1 : 0;
    my $any_in_range = 0;
    for my $match (@matches) {
        my $count = lines_in($match);
        my $in_range = defined($count) && $max_requested <= $count ? JSON::PP::true : JSON::PP::false;
        $all_in_range = 0 unless $in_range;
        $any_in_range = 1 if $in_range;
        push @checked, { file => $match, line_count => $count, in_range => $in_range };
    }
    push @results, {
        citation => "$path:$spec",
        path => $path,
        line_spec => $spec,
        resolution => $resolution,
        max_requested_line => $max_requested,
        any_match_in_range => $any_in_range ? JSON::PP::true : JSON::PP::false,
        all_matches_in_range => $all_in_range ? JSON::PP::true : JSON::PP::false,
        matches => \@checked,
    };
}

my %summary;
for my $r (@results) {
    $summary{$r->{resolution}}++;
    $summary{NO_CANDIDATE_IN_RANGE}++ unless $r->{any_match_in_range} || $r->{resolution} eq 'MISSING';
    $summary{AMBIGUOUS_WITH_SOME_OUT_OF_RANGE}++
        if $r->{resolution} =~ /^AMBIGUOUS/ && $r->{any_match_in_range} && !$r->{all_matches_in_range};
}

my $payload = {
    verifier => 'independent lexical path and line-range check',
    repository => $repo,
    report => $report,
    unique_file_line_citations => scalar(@results),
    unique_context_only_line_fragments => scalar(@bare),
    context_only_line_fragments => \@bare,
    summary => \%summary,
    results => \@results,
};
print JSON::PP->new->canonical->pretty->encode($payload);
